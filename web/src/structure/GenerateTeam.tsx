import { useEffect, useMemo, useRef, useState } from 'react';
import { api, ApiError, newRequestId, type GenerateOptions, type GeneratedTeam } from '../api';
import type { TeamDraft } from '../generated/TeamDraft';
import { errorText } from '../errors';
import type { DraftIssue } from '../generated/DraftIssue';
import { gw } from '../hosted/session';
import { CLI_NAME, DescribeForm } from './GenerateForm';
import { ChooseStep, ErrorBox, QuestionsStep, RefineStep, WaitingStep, type GenError, type Question } from './GenerateSteps';
import '../features.css';

type Step = 'describe' | 'questions' | 'waiting' | 'choose' | 'refine';
type Opts = GenerateOptions;
type SetOpts = (f: (o: Opts) => Opts) => void;

/** The team with only the kept agents and arrows (an arrow goes when either end does). */
function trimmed(t: TeamDraft, agents: Set<string>, arrows: Set<string>): TeamDraft {
  const kept = t.agents.filter((a) => agents.has(a.id));
  const groups = t.groups.filter((g) => kept.some((a) => a.group_id === g.id));
  const connections = t.connections.filter((c) => arrows.has(c.id) && agents.has(c.from) && agents.has(c.to));
  return { ...t, agents: kept, groups, connections };
}

/** Several connected computers: say which one runs the CLI (names from your account when hosted). */
function useComputers(setOpts: SetOpts) {
  const [computers, setComputers] = useState<{ id: string; name: string }[]>([]);
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
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return computers;
}

/** Which CLIs are signed in on the chosen computer; if Claude is not, the first that is becomes the choice. */
function useSignedIn(host: string | null | undefined, setOpts: SetOpts) {
  const [signedIn, setSignedIn] = useState<string[] | null>(null);
  useEffect(() => {
    let live = true;
    api.clis(host ?? undefined).then(
      (list) => {
        if (!live) return;
        const ok = list.filter((c) => c.signed_in).map((c) => c.name);
        setSignedIn(ok);
        if (ok.length > 0 && !ok.includes('claude')) setOpts((o) => ({ ...o, cli: ok[0] as Opts['cli'] }));
      },
      () => live && setSignedIn([]),
    );
    return () => {
      live = false;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host]);
  return signedIn;
}

/** Live issues for what would be created. */
function useLiveIssues(current: TeamDraft | null, setIssues: (i: DraftIssue[]) => void) {
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
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current]);
}

/** What the person has kept of the chosen team, and the issues it would have. */
function useRefine(setStep: (s: Step) => void) {
  const [chosen, setChosen] = useState<GeneratedTeam | null>(null);
  const [keepAgents, setKeepAgents] = useState<Set<string>>(new Set());
  const [keepArrows, setKeepArrows] = useState<Set<string>>(new Set());
  const [issues, setIssues] = useState<DraftIssue[]>([]);
  const choose = (t: GeneratedTeam) => {
    setChosen(t);
    setKeepAgents(new Set(t.draft.agents.map((a) => a.id)));
    setKeepArrows(new Set(t.draft.connections.map((c) => c.id)));
    setIssues(t.issues);
    setStep('refine');
  };
  const current = useMemo(() => (chosen ? trimmed(chosen.draft, keepAgents, keepArrows) : null), [chosen, keepAgents, keepArrows]);
  useLiveIssues(current, setIssues);
  return { chosen, keepAgents, setKeepAgents, keepArrows, setKeepArrows, issues, choose, current };
}

/** Busy, the last error (a cancel is not one) and the abort handle of the request in flight. */
function useRequestState() {
  const [error, setError] = useState<GenError | null>(null);
  const [busy, setBusy] = useState(false);
  const abort = useRef<AbortController | null>(null);
  useEffect(() => () => abort.current?.abort(), []);
  const fail = (e: unknown) => {
    if (e instanceof DOMException && e.name === 'AbortError') return;
    setError({ message: errorText(e), raw: e instanceof ApiError ? e.raw : undefined });
  };
  /** A request starts: no error, busy. */
  const begin = () => { setError(null); setBusy(true); };
  return { error, setError, busy, setBusy, abort, fail, begin };
}

/** The answers worth sending: those given, with the question they answer. */
const answered = (questions: Question[], answers: Record<number, string>) =>
  questions.map((q, i) => ({ question: q.question, answer: answers[i] ?? '' })).filter((a) => a.answer.trim());

/** Asking the CLI: questions first (optional), then 1 or 3 alternatives; a cancel aborts the request. */
function useAsking(req: ReturnType<typeof useRequestState>, opts: Opts, prompt: string, step: Step, setStep: (s: Step) => void, choose: (t: GeneratedTeam) => void) {
  const [questions, setQuestions] = useState<Question[]>([]);
  const [answers, setAnswers] = useState<Record<number, string>>({});
  const [offer, setOffer] = useState<GeneratedTeam[]>([]);
  const [count, setCount] = useState<1 | 3>(3);
  // "Same as the CLI" means agents of the CLI's own kind.
  const sent = (): Opts => ({ ...opts, kinds: opts.kinds ?? opts.cli });
  const ask = async () => {
    req.begin();
    req.abort.current = new AbortController();
    try {
      const r = await api.generateQuestions({ options: sent(), prompt }, req.abort.current.signal);
      setQuestions(r.questions);
      setAnswers({});
      setStep('questions');
    } catch (e) {
      req.fail(e);
    } finally {
      req.setBusy(false);
    }
  };
  const generate = async (withAnswers: boolean) => {
    req.begin();
    const before = step;
    setStep('waiting');
    req.abort.current = new AbortController();
    try {
      const qa = withAnswers ? answered(questions, answers) : [];
      const r = await api.generateTeams({ options: sent(), prompt, answers: qa, count }, req.abort.current.signal);
      setOffer(r.alternatives);
      if (r.alternatives.length === 1) choose(r.alternatives[0]);
      else setStep('choose');
    } catch (e) {
      req.fail(e);
      setStep(before);
    } finally {
      req.setBusy(false);
    }
  };
  return { questions, answers, setAnswer: (i: number, a: string) => setAnswers((cur) => ({ ...cur, [i]: a })), offer, count, setCount, ask, generate };
}

/**
 * "Describe it": your own Claude Code or Codex drafts a team. Optional clarifying questions,
 * 1 or 3 alternatives, then keep or drop each agent and arrow before the team is created.
 */
export function GenerateTeam({ onCreated, onCancel }: { onCreated: (id: string) => void; onCancel: () => void }) {
  const [step, setStep] = useState<Step>('describe');
  const [prompt, setPrompt] = useState('');
  const [opts, setOpts] = useState<Opts>({ cli: 'claude', model: null, size: '2-3', kinds: null, shape: null, checks: false, folder: null });
  const [askFirst, setAskFirst] = useState(true);
  const refine = useRefine(setStep);
  const req = useRequestState();
  const q = useAsking(req, opts, prompt, step, setStep, refine.choose);
  const computers = useComputers(setOpts);
  const signedIn = useSignedIn(opts.host, setOpts);
  const cliName = CLI_NAME[opts.cli];
  const create = async () => {
    if (!refine.current) return;
    req.begin();
    try {
      const origin = { kind: 'generated', cli: opts.cli, model: opts.model, at_ms: Date.now(), prompt };
      const r = await api.createDraft(refine.current, newRequestId(), origin);
      onCreated(r.team.draft.id);
    } catch (e) {
      req.fail(e);
    } finally {
      req.setBusy(false);
    }
  };
  const errorBox = <ErrorBox error={req.error} cli={opts.cli} />;
  const set = <K extends keyof Opts>(k: K, v: Opts[K]) => setOpts((o) => ({ ...o, [k]: v }));
  return (
    <section className="gen" aria-label="Describe a team">
      {step === 'describe' && <DescribeForm prompt={prompt} setPrompt={setPrompt} busy={req.busy} errorBox={errorBox} onCancel={onCancel} onSubmit={() => void (askFirst ? q.ask() : q.generate(false))}
        opts={opts} set={set} computers={computers} signedIn={signedIn} count={q.count} setCount={q.setCount} askFirst={askFirst} setAskFirst={setAskFirst} />}
      {step === 'questions' && <QuestionsStep questions={q.questions} answers={q.answers} setAnswer={q.setAnswer} generate={(w) => void q.generate(w)} errorBox={errorBox} />}
      {step === 'waiting' && <WaitingStep cliName={cliName} onCancel={() => req.abort.current?.abort()} />}
      {step === 'choose' && <ChooseStep offer={q.offer} choose={refine.choose} onBack={() => setStep('describe')} />}
      {step === 'refine' && refine.chosen && refine.current && (
        <RefineStep chosen={refine.chosen} current={refine.current} keepAgents={refine.keepAgents} keepArrows={refine.keepArrows} setKeepAgents={refine.setKeepAgents} setKeepArrows={refine.setKeepArrows}
          issues={refine.issues} errorBox={errorBox} busy={req.busy} back={() => setStep(q.offer.length > 1 ? 'choose' : 'describe')} create={() => void create()} />
      )}
    </section>
  );
}
