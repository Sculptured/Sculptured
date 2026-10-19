import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { api, ApiError, newRequestId, type GenerateOptions, type GeneratedTeam } from '../api';
import type { TeamDraft } from '../generated/TeamDraft';
import { errorText } from '../errors';
import type { DraftIssue } from '../generated/DraftIssue';
import { Btn } from '../shell/ui';
import { TextArea } from '../shell/TextArea';
import { TemplatePreview } from './TemplateGallery';
import { plural } from '../format';
import { gw } from '../hosted/session';
import '../features.css';

type Step = 'describe' | 'questions' | 'waiting' | 'choose' | 'refine';
type Question = { question: string; suggestions: string[] };

const CLI_NAME: Record<string, string> = { claude: 'Claude Code', codex: 'Codex' };
const LABELS: Record<string, string> = { lean: 'Lean', balanced: 'Balanced', thorough: 'Thorough', team: 'Team' };

/** The team with only the kept agents and arrows (an arrow goes when either end does). */
function trimmed(t: TeamDraft, agents: Set<string>, arrows: Set<string>): TeamDraft {
  const kept = t.agents.filter((a) => agents.has(a.id));
  const groups = t.groups.filter((g) => kept.some((a) => a.group_id === g.id));
  const connections = t.connections.filter((c) => arrows.has(c.id) && agents.has(c.from) && agents.has(c.to));
  return { ...t, agents: kept, groups, connections };
}

/**
 * "Describe it": your own Claude Code or Codex drafts a team. Optional clarifying questions,
 * 1 or 3 alternatives, then keep or drop each agent and arrow before the team is created.
 */
export function GenerateTeam({ onCreated, onCancel }: { onCreated: (id: string) => void; onCancel: () => void }) {
  const ids = useId();
  const [step, setStep] = useState<Step>('describe');
  const [prompt, setPrompt] = useState('');
  const [signedIn, setSignedIn] = useState<string[] | null>(null);
  const [computers, setComputers] = useState<{ id: string; name: string }[]>([]);
  const [opts, setOpts] = useState<GenerateOptions>({ cli: 'claude', model: null, size: '2-3', kinds: null, shape: null, checks: false, folder: null });
  const [askFirst, setAskFirst] = useState(true);
  const [count, setCount] = useState<1 | 3>(3);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [answers, setAnswers] = useState<Record<number, string>>({});
  const [offer, setOffer] = useState<GeneratedTeam[]>([]);
  const [chosen, setChosen] = useState<GeneratedTeam | null>(null);
  const [keepAgents, setKeepAgents] = useState<Set<string>>(new Set());
  const [keepArrows, setKeepArrows] = useState<Set<string>>(new Set());
  const [issues, setIssues] = useState<DraftIssue[]>([]);
  const [error, setError] = useState<{ message: string; raw?: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const abort = useRef<AbortController | null>(null);
  const cliName = CLI_NAME[opts.cli];

  // Several connected computers: say which one runs the CLI (names from your account when hosted).
  useEffect(() => {
    let live = true;
    api.hosts().then(
      async (hs) => {
        if (!live || hs.length < 2) return;
        let names: Record<string, string> = {};
        try {
          const devices = await gw<{ id: string; name: string }[]>('GET', '/devices');
          names = Object.fromEntries(devices.map((d) => [d.id, d.name]));
        } catch {
          // Not hosted, or the list is unavailable: ids will do.
        }
        if (!live) return;
        setComputers(hs.map((h) => ({ id: h.id, name: names[h.id] ?? h.id })));
        setOpts((o) => ({ ...o, host: o.host ?? hs[0].id }));
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    let live = true;
    api.clis(opts.host ?? undefined).then(
      (list) => {
        if (!live) return;
        const ok = list.filter((c) => c.signed_in).map((c) => c.name);
        setSignedIn(ok);
        if (ok.length > 0 && !ok.includes('claude')) setOpts((o) => ({ ...o, cli: ok[0] as GenerateOptions['cli'] }));
      },
      () => live && setSignedIn([]),
    );
    return () => {
      live = false;
    };
  }, [opts.host]);

  useEffect(() => () => abort.current?.abort(), []);

  // "Same as the CLI" means agents of the CLI's own kind.
  const sent = (): GenerateOptions => ({ ...opts, kinds: opts.kinds ?? opts.cli });

  const fail = (e: unknown) => {
    if (e instanceof DOMException && e.name === 'AbortError') return;
    setError({ message: errorText(e), raw: e instanceof ApiError ? e.raw : undefined });
  };

  const choose = (t: GeneratedTeam) => {
    setChosen(t);
    setKeepAgents(new Set(t.draft.agents.map((a) => a.id)));
    setKeepArrows(new Set(t.draft.connections.map((c) => c.id)));
    setIssues(t.issues);
    setStep('refine');
  };

  const ask = async () => {
    setError(null);
    setBusy(true);
    abort.current = new AbortController();
    try {
      const r = await api.generateQuestions({ options: sent(), prompt }, abort.current.signal);
      setQuestions(r.questions);
      setAnswers({});
      setStep('questions');
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const generate = async (withAnswers: boolean) => {
    setError(null);
    setBusy(true);
    const before = step;
    setStep('waiting');
    abort.current = new AbortController();
    try {
      const qa = withAnswers ? questions.map((q, i) => ({ question: q.question, answer: answers[i] ?? '' })).filter((a) => a.answer.trim()) : [];
      const r = await api.generateTeams({ options: sent(), prompt, answers: qa, count }, abort.current.signal);
      setOffer(r.alternatives);
      if (r.alternatives.length === 1) choose(r.alternatives[0]);
      else setStep('choose');
    } catch (e) {
      fail(e);
      setStep(before);
    } finally {
      setBusy(false);
    }
  };

  const current = useMemo(() => (chosen ? trimmed(chosen.draft, keepAgents, keepArrows) : null), [chosen, keepAgents, keepArrows]);

  // Live issues for what would be created.
  useEffect(() => {
    if (!current) return;
    let live = true;
    const t = window.setTimeout(() => {
      api.checkDraft(current).then((r) => live && setIssues(r.issues), (e) => live && setIssues(e instanceof ApiError ? (e.issues ?? []) : []));
    }, 300);
    return () => {
      live = false;
      window.clearTimeout(t);
    };
  }, [current]);

  const create = async () => {
    if (!current) return;
    setError(null);
    setBusy(true);
    try {
      const origin = { kind: 'generated', cli: opts.cli, model: opts.model, at_ms: Date.now(), prompt };
      const r = await api.createDraft(current, newRequestId(), origin);
      onCreated(r.team.draft.id);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const errorBox = error && (
    <div role="alert" className="gen-error tone-red">
      <p>{error.message}</p>
      {error.raw && (
        <details>
          <summary>What {cliName} answered</summary>
          <pre>{error.raw}</pre>
        </details>
      )}
    </div>
  );

  const set = <K extends keyof GenerateOptions>(k: K, v: GenerateOptions[K]) => setOpts((o) => ({ ...o, [k]: v }));

  return (
    <section className="gen" aria-label="Describe a team">
      {step === 'describe' && (
        <form
          className="gen-describe"
          onSubmit={(e) => {
            e.preventDefault();
            void (askFirst ? ask() : generate(false));
          }}
        >
          <label htmlFor={`${ids}-prompt`} className="gen-label">
            Describe the team
          </label>
          <TextArea id={`${ids}-prompt`} className="fld-input" rows={4} value={prompt} placeholder="e.g. Rebuild our docs site in Astro: one agent plans the pages, two write them, one reviews" onChange={(e) => setPrompt(e.target.value)} />
          <details className="gen-options">
            <summary>Options</summary>
            <div className="gen-grid">
              {computers.length > 1 && (
                <label>
                  Computer{' '}
                  <select value={opts.host ?? ''} onChange={(e) => set('host', e.target.value)}>
                    {computers.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <label>
                CLI{' '}
                <select value={opts.cli} onChange={(e) => set('cli', e.target.value as GenerateOptions['cli'])}>
                  {(['claude', 'codex'] as const).map((c) => (
                    <option key={c} value={c}>
                      {CLI_NAME[c]}
                      {signedIn && !signedIn.includes(c) ? ' (not signed in)' : ''}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Model <input className="fld-input" value={opts.model ?? ''} placeholder="the CLI's default" onChange={(e) => set('model', e.target.value.trim() || null)} />
              </label>
              <label>
                Team size{' '}
                <select value={opts.size ?? ''} onChange={(e) => set('size', (e.target.value || null) as GenerateOptions['size'])}>
                  <option value="2-3">2–3 agents</option>
                  <option value="4-6">4–6 agents</option>
                  <option value="7-10">7–10 agents</option>
                </select>
              </label>
              <label>
                Agent kinds{' '}
                <select value={opts.kinds ?? ''} onChange={(e) => set('kinds', (e.target.value || null) as GenerateOptions['kinds'])}>
                  <option value="">Same as the CLI</option>
                  <option value="claude">Claude</option>
                  <option value="codex">Codex</option>
                  <option value="mixed">Mixed</option>
                  <option value="practice">Practice (free, no model calls when run)</option>
                </select>
              </label>
              <label>
                Shape{' '}
                <select value={opts.shape ?? ''} onChange={(e) => set('shape', (e.target.value || null) as GenerateOptions['shape'])}>
                  <option value="">Let the AI choose</option>
                  <option value="chain">A chain</option>
                  <option value="lead">A lead with helpers</option>
                  <option value="reviewers">Builders with reviewers</option>
                </select>
              </label>
              <label>
                Alternatives{' '}
                <select value={count} onChange={(e) => setCount(e.target.value === '1' ? 1 : 3)}>
                  <option value="3">3</option>
                  <option value="1">1</option>
                </select>
              </label>
              <label className="gen-check">
                <input type="checkbox" checked={opts.checks} onChange={(e) => set('checks', e.target.checked)} /> Add check commands
              </label>
              <label className="gen-check">
                <input type="checkbox" checked={askFirst} onChange={(e) => setAskFirst(e.target.checked)} /> Ask me questions first
              </label>
            </div>
          </details>
          <p className="muted gen-cost">
            Uses your {cliName} plan · {opts.model ?? 'default model'}.{' '}
            {opts.cli === 'claude'
              ? 'Claude Code runs with no tools or MCP servers: it sees only this description.'
              : 'Codex runs read-only: it can read files on this computer but not change them.'}
          </p>
          {errorBox}
          <div className="gen-actions">
            <Btn onClick={onCancel}>Cancel</Btn>
            <Btn type="submit" variant="primary" disabled={!prompt.trim() || busy}>
              {askFirst ? 'Next: questions' : 'Generate'}
            </Btn>
          </div>
          {busy && <p role="status">Asking {cliName}… this usually takes under a minute.</p>}
        </form>
      )}

      {step === 'questions' && (
        <div className="gen-questions">
          {questions.map((q, i) => (
            <fieldset key={i}>
              <legend>{q.question}</legend>
              {q.suggestions.map((s) => (
                <label key={s} className="gen-check">
                  <input type="radio" name={`${ids}-q${i}`} checked={answers[i] === s} onChange={() => setAnswers((a) => ({ ...a, [i]: s }))} /> {s}
                </label>
              ))}
              <input className="fld-input" aria-label={`Other answer to: ${q.question}`} placeholder="Other…" value={q.suggestions.includes(answers[i] ?? '') ? '' : (answers[i] ?? '')} onChange={(e) => setAnswers((a) => ({ ...a, [i]: e.target.value }))} />
            </fieldset>
          ))}
          {errorBox}
          <div className="gen-actions">
            <Btn onClick={() => void generate(false)}>Skip all</Btn>
            <Btn variant="primary" onClick={() => void generate(true)}>
              Generate
            </Btn>
          </div>
        </div>
      )}

      {step === 'waiting' && (
        <div className="gen-waiting">
          <p role="status">Asking {cliName}… this usually takes under a minute.</p>
          <Btn onClick={() => abort.current?.abort()}>Cancel</Btn>
        </div>
      )}

      {step === 'choose' && (
        <div className="gen-choose">
          {offer.map((t) => (
            <article key={t.label} className="gen-card">
              <h3>{LABELS[t.label] ?? t.label}</h3>
              <TemplatePreview draft={t.draft} practice={t.draft.agents.every((a) => a.kind === 'practice')} />
              <p className="muted">
                {plural(t.draft.agents.length, 'agent')} · {plural(t.draft.connections.length, 'arrow')}
                {t.issues.length > 0 ? ` · ${plural(t.issues.length, 'issue')}` : ''}
              </p>
              <p>{t.rationale}</p>
              <Btn variant="primary" aria-label={`Use ${t.label}`} onClick={() => choose(t)}>
                Use this
              </Btn>
            </article>
          ))}
          <div className="gen-actions">
            <Btn onClick={() => setStep('describe')}>Back</Btn>
          </div>
        </div>
      )}

      {step === 'refine' && chosen && current && (
        <div className="gen-refine">
          <TemplatePreview draft={current} practice={current.agents.every((a) => a.kind === 'practice')} />
          <fieldset>
            <legend>Agents</legend>
            {chosen.draft.agents.map((a) => (
              <label key={a.id} className="gen-check gen-item">
                <input
                  type="checkbox"
                  checked={keepAgents.has(a.id)}
                  onChange={(e) =>
                    setKeepAgents((s) => {
                      const n = new Set(s);
                      if (e.target.checked) n.add(a.id);
                      else n.delete(a.id);
                      return n;
                    })
                  }
                />
                <span>
                  {a.name} <span className="muted">· {a.kind}</span>
                  <span className="gen-job">{a.job}</span>
                </span>
              </label>
            ))}
          </fieldset>
          {chosen.draft.connections.length > 0 && (
            <fieldset>
              <legend>Arrows</legend>
              {chosen.draft.connections.map((c) => {
                const ends = keepAgents.has(c.from) && keepAgents.has(c.to);
                const name = (id: string) => chosen.draft.agents.find((a) => a.id === id)?.name ?? id;
                const verb = c.kind === 'hands-work-to' ? 'hands work to' : c.kind === 'reports-to' ? 'reports to' : 'watches';
                return (
                  <label key={c.id} className="gen-check gen-item">
                    <input
                      type="checkbox"
                      disabled={!ends}
                      checked={ends && keepArrows.has(c.id)}
                      onChange={(e) =>
                        setKeepArrows((s) => {
                          const n = new Set(s);
                          if (e.target.checked) n.add(c.id);
                          else n.delete(c.id);
                          return n;
                        })
                      }
                    />
                    {`${name(c.from)} ${verb} ${name(c.to)}`}
                  </label>
                );
              })}
            </fieldset>
          )}
          {chosen.dropped.length > 0 && (
            <ul className="muted" aria-label="Left out of the draft">
              {chosen.dropped.map((d, i) => (
                <li key={i}>{d}</li>
              ))}
            </ul>
          )}
          {issues.length > 0 && (
            <ul className="gen-issues" aria-label="Issues">
              {issues.map((i, n) => (
                <li key={n}>{i.message}</li>
              ))}
            </ul>
          )}
          {errorBox}
          <div className="gen-actions">
            <Btn onClick={() => setStep(offer.length > 1 ? 'choose' : 'describe')}>Back</Btn>
            <Btn variant="primary" disabled={busy || keepAgents.size === 0} onClick={() => void create()}>
              Create team
            </Btn>
          </div>
        </div>
      )}
    </section>
  );
}
