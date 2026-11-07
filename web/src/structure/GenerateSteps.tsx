import { useId } from 'react';
import type { GeneratedTeam } from '../api';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import { Btn } from '../shell/ui';
import { TemplatePreview } from './TemplatePreview';
import { plural } from '../format';
import { CLI_NAME } from './GenerateForm';

export type Question = { question: string; suggestions: string[] };
export type GenError = { message: string; raw?: string };

const LABELS: Record<string, string> = { lean: 'Lean', balanced: 'Balanced', thorough: 'Thorough', team: 'Team' };

export function ErrorBox({ error, cli }: { error: GenError | null; cli: string }) {
  if (!error) return null;
  return (
    <div role="alert" className="gen-error tone-red">
      <p>{error.message}</p>
      {error.raw && (
        <details>
          <summary>What {CLI_NAME[cli]} answered</summary>
          <pre>{error.raw}</pre>
        </details>
      )}
    </div>
  );
}

type QuestionsProps = { questions: Question[]; answers: Record<number, string>; setAnswer: (i: number, a: string) => void; generate: (withAnswers: boolean) => void; errorBox: React.ReactNode };

export function QuestionsStep({ questions, answers, setAnswer, generate, errorBox }: QuestionsProps) {
  const ids = useId();
  return (
    <div className="gen-questions">
      {questions.map((q, i) => (
        <fieldset key={i}>
          <legend>{q.question}</legend>
          {q.suggestions.map((s) => (
            <label key={s} className="gen-check">
              <input type="radio" name={`${ids}-q${i}`} checked={answers[i] === s} onChange={() => setAnswer(i, s)} /> {s}
            </label>
          ))}
          <input className="fld-input" aria-label={`Other answer to: ${q.question}`} placeholder="Other…" value={q.suggestions.includes(answers[i] ?? '') ? '' : (answers[i] ?? '')} onChange={(e) => setAnswer(i, e.target.value)} />
        </fieldset>
      ))}
      {errorBox}
      <div className="gen-actions">
        <Btn onClick={() => generate(false)}>Skip all</Btn>
        <Btn variant="primary" onClick={() => generate(true)}>
          Generate
        </Btn>
      </div>
    </div>
  );
}

export function WaitingStep({ cliName, onCancel }: { cliName: string; onCancel: () => void }) {
  return (
    <div className="gen-waiting">
      <p role="status">Asking {cliName}… this usually takes under a minute.</p>
      <Btn onClick={onCancel}>Cancel</Btn>
    </div>
  );
}

export function ChooseStep({ offer, choose, onBack }: { offer: GeneratedTeam[]; choose: (t: GeneratedTeam) => void; onBack: () => void }) {
  return (
    <div className="gen-choose">
      {offer.map((t) => (
        <article key={t.label} className="gen-card">
          <h3>{LABELS[t.label] ?? t.label}</h3>
          <TemplatePreview draft={t.draft} />
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
        <Btn onClick={onBack}>Back</Btn>
      </div>
    </div>
  );
}

const toggled = (s: Set<string>, id: string, on: boolean) => {
  const n = new Set(s);
  if (on) n.add(id);
  else n.delete(id);
  return n;
};

const VERB: Record<string, string> = { 'hands-work-to': 'hands work to', 'reports-to': 'reports to', watches: 'watches' };

type KeepProps = { chosen: GeneratedTeam; keepAgents: Set<string>; keepArrows: Set<string>; setKeepAgents: (f: (s: Set<string>) => Set<string>) => void; setKeepArrows: (f: (s: Set<string>) => Set<string>) => void };

function KeepAgents({ chosen, keepAgents, setKeepAgents }: Pick<KeepProps, 'chosen' | 'keepAgents' | 'setKeepAgents'>) {
  return (
    <fieldset>
      <legend>Agents</legend>
      {chosen.draft.agents.map((a) => (
        <label key={a.id} className="gen-check gen-item">
          <input type="checkbox" checked={keepAgents.has(a.id)} onChange={(e) => setKeepAgents((s) => toggled(s, a.id, e.target.checked))} />
          <span>
            {a.name} <span className="muted">· {a.kind}</span>
            <span className="gen-job">{a.job}</span>
          </span>
        </label>
      ))}
    </fieldset>
  );
}

function KeepArrows({ chosen, keepAgents, keepArrows, setKeepArrows }: Omit<KeepProps, 'setKeepAgents'>) {
  if (chosen.draft.connections.length === 0) return null;
  const name = (id: string) => chosen.draft.agents.find((a) => a.id === id)?.name ?? id;
  return (
    <fieldset>
      <legend>Arrows</legend>
      {chosen.draft.connections.map((c) => {
        const ends = keepAgents.has(c.from) && keepAgents.has(c.to);
        return (
          <label key={c.id} className="gen-check gen-item">
            <input type="checkbox" disabled={!ends} checked={ends && keepArrows.has(c.id)} onChange={(e) => setKeepArrows((s) => toggled(s, c.id, e.target.checked))} />
            {`${name(c.from)} ${VERB[c.kind] ?? 'watches'} ${name(c.to)}`}
          </label>
        );
      })}
    </fieldset>
  );
}

type RefineProps = KeepProps & { current: TeamDraft; issues: DraftIssue[]; errorBox: React.ReactNode; busy: boolean; back: () => void; create: () => void };

export function RefineStep({ chosen, current, keepAgents, keepArrows, setKeepAgents, setKeepArrows, issues, errorBox, busy, back, create }: RefineProps) {
  return (
    <div className="gen-refine">
      <TemplatePreview draft={current} />
      <KeepAgents chosen={chosen} keepAgents={keepAgents} setKeepAgents={setKeepAgents} />
      <KeepArrows chosen={chosen} keepAgents={keepAgents} keepArrows={keepArrows} setKeepArrows={setKeepArrows} />
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
        <Btn onClick={back}>Back</Btn>
        <Btn variant="primary" disabled={busy || keepAgents.size === 0} onClick={create}>
          Create team
        </Btn>
      </div>
    </div>
  );
}
