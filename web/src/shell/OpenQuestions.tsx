import { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { Answer } from '../generated/Answer';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import { usePolled } from '../usePolling';

/** A question that needs the person: open, an answer that may not have arrived, or one that timed out unseen. */
export function needsYou(q: QuestionView): boolean {
  if (q.state === 'open') return true;
  if (q.state === 'answered') return q.unconfirmed !== null;
  return q.state === 'expired' && !q.acknowledged && q.late_answer === null;
}

const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

type Props = {
  seat: string;
  name: string;
  feed: EventRecord[];
  /** Changes when the person chose "Answer": the first question takes the focus. */
  focusToken?: number;
};

/**
 * Above the agent's conversation: the questions and tool approvals it is waiting on, answered here
 * (an option, words, Allow or Deny). Also an answer that may not have reached it ("Send again"), and a
 * question nobody answered in time, which can still get a late answer (it overrides what the agent decided).
 */
export function OpenQuestions({ seat, name, feed, focusToken = 0 }: Props) {
  // Refetch as soon as a question event of this agent arrives; poll slowly besides.
  const changes = feed.filter((e) => e.subject.kind === 'seat' && e.subject.id === seat && e.body.type.startsWith('question')).length;
  const { data, reload } = usePolled(() => api.seatQuestions(seat), 15_000, [seat, changes]);
  const shown = (data ?? []).filter(needsYou);
  const box = useRef<HTMLElement>(null);
  const focused = useRef(0);
  useEffect(() => {
    if (focusToken === focused.current || shown.length === 0) return;
    focused.current = focusToken;
    box.current?.querySelector<HTMLElement>('textarea, input, button')?.focus();
  });
  if (shown.length === 0) return null;
  return (
    <section ref={box} className="open-questions" aria-label={`Waiting for you: ${name}`}>
      {shown.map((q) => <QuestionCard key={q.id} q={q} name={name} onDone={reload} />)}
    </section>
  );
}

/** What a card's buttons share: the text typed, a call in flight, and its error. */
type Act = {
  q: QuestionView;
  name: string;
  text: string;
  setText: (t: string) => void;
  busy: boolean;
  run: (call: () => Promise<unknown>) => Promise<void>;
};

export function QuestionCard({ q, name, onDone }: { q: QuestionView; name: string; onDone: () => void }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const run = async (call: () => Promise<unknown>) => {
    setBusy(true);
    setError('');
    await call().catch((e: unknown) => setError(errorText(e)));
    setBusy(false);
    onDone();
  };
  const act: Act = { q, name, text, setText, busy, run };
  const permission = q.source === 'native-permission';
  return (
    <article className={`question-card${permission ? ' permission' : ''}`}>
      <Asked q={q} name={name} />
      {q.state === 'answered' ? <Unconfirmed {...act} /> : q.state === 'expired' ? <Late {...act} /> : permission ? <Approval {...act} /> : <Reply {...act} />}
      {q.state === 'open' && q.deadline_ms !== null && (
        <p className="muted question-deadline">
          {permission ? `Allowed automatically at ${clock(q.deadline_ms)} if nobody answers.` : `${name} decides on its own at ${clock(q.deadline_ms)} if nobody answers.`}
        </p>
      )}
      {error && <p className="tone-red" role="alert">{error}</p>}
    </article>
  );
}

/** What the agent asked, verbatim; for a tool approval, the tool, its input and what it would do. */
function Asked({ q, name }: { q: QuestionView; name: string }) {
  const permission = q.source === 'native-permission';
  const title = permission ? `${name} wants to run ${q.prompt.tool ?? 'a tool'}` : q.prompt.header ?? `${name} asks`;
  return (
    <>
      <p className="question-title"><strong>{title}</strong>{q.prompt.urgency === 'high' && <>{' · '}<span className="tone-amber question-urgency">Urgent, says the agent</span></>}</p>
      <p className="question-text" data-raw-evidence>{q.prompt.text}</p>
      {permission && q.consequence.length > 0 && (
        <ul className="question-risks" aria-label="What it would do (Sculptured's reading)">
          {q.consequence.map((r) => <li key={r.code}>{r.text}</li>)}
        </ul>
      )}
      {permission && q.prompt.tool_input !== null && (
        <details className="question-input">
          <summary>Tool input</summary>
          <pre data-raw-evidence>{JSON.stringify(q.prompt.tool_input, null, 2)}</pre>
        </details>
      )}
    </>
  );
}

const reply = (a: Act, answer: Answer) => void a.run(() => api.answer(a.q.id, answer));
const reasonOf = (a: Act) => a.text.trim() || null;

/** An answer was sent but may not have reached the agent. */
function Unconfirmed(a: Act) {
  return (
    <>
      <p className="tone-amber">Your answer may not have reached {a.name}: <span data-raw-evidence>{a.q.unconfirmed}</span></p>
      <p className="question-actions">
        <button type="button" className="btn btn-primary" disabled={a.busy} onClick={() => void a.run(() => api.sendAgain(a.q.id))}>Send again</button>
      </p>
    </>
  );
}

/** Nobody answered in time: deny the approval now, or send a late answer that overrides the agent's decision. */
function Late(a: Act) {
  const permission = a.q.source === 'native-permission';
  const words = a.text.trim();
  return (
    <>
      <p className="muted">
        Nobody answered in time.{' '}
        {permission ? 'It was allowed automatically.' : <>{a.name} decided: <span data-raw-evidence>{a.q.decision?.text ?? 'unknown'}</span></>}
      </p>
      {permission ? <ReasonBox {...a} /> : <AnswerBox label="A late answer (it overrides that decision)" {...a} />}
      <p className="question-actions">
        {permission ? (
          <button type="button" className="btn btn-danger" disabled={a.busy} onClick={() => reply(a, { kind: 'deny', reason: reasonOf(a) })}>Deny now</button>
        ) : (
          <button type="button" className="btn btn-primary" disabled={a.busy || !words} onClick={() => reply(a, { kind: 'text', text: words })}>Send late answer</button>
        )}
        <button type="button" className="btn" disabled={a.busy} onClick={() => void a.run(() => api.acknowledge(a.q.id))}>OK</button>
      </p>
    </>
  );
}

/** A tool approval: Allow, or Deny with an optional reason the agent is told. */
function Approval(a: Act) {
  return (
    <>
      <ReasonBox {...a} />
      <p className="question-actions">
        <button type="button" className="btn btn-primary" disabled={a.busy} onClick={() => reply(a, { kind: 'allow' })}>Allow</button>
        <button type="button" className="btn btn-danger" disabled={a.busy} onClick={() => reply(a, { kind: 'deny', reason: reasonOf(a) })}>Deny</button>
      </p>
    </>
  );
}

/** A question: one of its options, or words when it takes them. */
function Reply(a: Act) {
  const { options, allow_other } = a.q.prompt;
  const words = a.text.trim();
  return (
    <>
      {options.length > 0 && (
        <p className="question-actions">
          {options.map((o) => (
            <button key={o.label} type="button" className="btn" disabled={a.busy} title={o.description ?? undefined} onClick={() => reply(a, { kind: 'text', text: o.label })}>{o.label}</button>
          ))}
        </p>
      )}
      {(allow_other || options.length === 0) && (
        <>
          <AnswerBox label={`Your answer to ${a.name}`} {...a} />
          <p className="question-actions">
            <button type="button" className="btn btn-primary" disabled={a.busy || !words} onClick={() => reply(a, { kind: 'text', text: words })}>Send answer</button>
          </p>
        </>
      )}
    </>
  );
}

function AnswerBox({ label, text, setText }: Act & { label: string }) {
  return <textarea className="question-answer" aria-label={label} placeholder={label} rows={2} value={text} onChange={(e) => setText(e.target.value)} />;
}

function ReasonBox({ text, setText }: Act) {
  return <input className="question-reason" type="text" aria-label="Reason (optional)" placeholder="Reason, if you deny (optional)" value={text} onChange={(e) => setText(e.target.value)} />;
}
