import { useState } from 'react';
import { Link } from 'react-router-dom';
import type { Answer } from '../generated/Answer';
import type { QuestionView } from '../generated/QuestionView';
import { agentPagePath } from '../paths';
import { needsYou, QuestionCard } from '../shell/OpenQuestions';
import './questions.css';

/** What came of a question, for the filters. */
export type Outcome = 'waiting' | 'answered' | 'decided' | 'withdrawn';

export function outcomeOf(q: QuestionView): Outcome {
  if (needsYou(q)) return 'waiting';
  if (q.state === 'answered' || q.late_answer) return 'answered';
  if (q.state === 'expired') return 'decided';
  return 'withdrawn';
}

const FILTERS: { id: Outcome | 'all'; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'waiting', label: 'Waiting for you' },
  { id: 'answered', label: 'Answered' },
  { id: 'decided', label: 'Agent decided' },
  { id: 'withdrawn', label: 'Withdrawn' },
];

const when = (ms: number) => new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

function answerWords(a: Answer): string {
  switch (a.kind) {
    case 'text': return `You answered: ${a.text}`;
    case 'allow': return 'You allowed it';
    case 'deny': return `You denied it${a.reason ? `: ${a.reason}` : ''}`;
    default: return 'No answer';
  }
}

/** What came of it, in words: your answer, the agent's own decision, or why it was withdrawn. */
function Result({ q }: { q: QuestionView }) {
  if (q.state === 'withdrawn') return <p className="ql-result muted">Withdrawn: {q.withdrawn_reason ?? 'no reason given'}</p>;
  if (q.state === 'answered' && q.answer) {
    return <p className="ql-result">{answerWords(q.answer)}{q.answered_at_ms ? ` · ${when(q.answered_at_ms)}` : ''}</p>;
  }
  const d = q.decision;
  return <>
    <p className="ql-result">No answer in time. {d && d.text !== 'unknown'
      ? <span data-raw-evidence>The agent decided: {d.text}{d.rationale ? `, because ${d.rationale}` : ''} (agent-reported)</span>
      : 'What the agent decided is not known.'}</p>
    {q.late_answer && <p className="ql-result">Your late answer, which overrode it: {answerWords(q.late_answer).replace(/^You answered: /, '')}</p>}
  </>;
}

/** The choices it offered, with the one picked. */
function Choices({ q }: { q: QuestionView }) {
  if (!q.prompt.options.length) return null;
  const picked = [q.answer, q.late_answer].map((a) => (a?.kind === 'text' ? a.text : null));
  return <ul className="ql-choices" aria-label="Choices">
    {q.prompt.options.map((o) => <li key={o.label} className={picked.includes(o.label) ? 'picked' : undefined}>{o.label}{picked.includes(o.label) ? ' (your answer)' : ''}{o.description ? ` — ${o.description}` : ''}</li>)}
  </ul>;
}

function Entry({ q, team, name, showAgent, onChanged }: { q: QuestionView; team: string; name: string; showAgent: boolean; onChanged: () => void }) {
  const permission = q.source === 'native-permission';
  const label = q.prompt.header ?? q.prompt.text;
  return <li className={`ql-entry ql-${outcomeOf(q)}`} aria-label={label}>
    <p className="ql-meta">
      {showAgent && <><Link to={agentPagePath(team, q.seat)}>{name}</Link>{' · '}</>}
      {permission ? `wanted to run ${q.prompt.tool ?? 'a tool'}` : 'asked'} · {when(q.opened_at_ms)}
      {q.prompt.urgency === 'high' && <>{' · '}<span className="tone-amber">Urgent, says the agent</span></>}
    </p>
    {outcomeOf(q) === 'waiting' ? <QuestionCard q={q} name={name} onDone={onChanged} /> : <>
      {q.prompt.header && <p className="ql-header"><strong>{q.prompt.header}</strong></p>}
      <p className="ql-text" data-raw-evidence>{q.prompt.text}</p>
      <Choices q={q} />
      <Result q={q} />
    </>}
  </li>;
}

/**
 * Every question agents asked the person: open ones answered here, and the history of answers,
 * choices, the agents' own decisions when nobody answered, and withdrawn ones. Newest first.
 */
export function QuestionLog({ team, questions, names, showAgent, onChanged }: { team: string; questions: QuestionView[]; names: Record<string, string>; showAgent: boolean; onChanged: () => void }) {
  const [filter, setFilter] = useState<Outcome | 'all'>('all');
  const count = (id: Outcome | 'all') => (id === 'all' ? questions.length : questions.filter((q) => outcomeOf(q) === id).length);
  const shown = questions.filter((q) => filter === 'all' || outcomeOf(q) === filter);
  if (!questions.length) return <p className="muted ql-empty">No questions yet. When an agent asks you something, it shows here, with your answer afterwards.</p>;
  return <div className="question-log">
    <div className="ql-filters" role="group" aria-label="Show">
      {FILTERS.map((f) => <button key={f.id} type="button" className={`btn btn-sm${filter === f.id ? ' on' : ''}`} aria-pressed={filter === f.id} onClick={() => setFilter(f.id)}>{`${f.label} (${count(f.id)})`}</button>)}
    </div>
    <ol className="ql-list" aria-label="Questions">
      {shown.map((q) => <Entry key={q.id} q={q} team={team} name={names[q.seat] ?? q.seat.split('@')[0]} showAgent={showAgent} onChanged={onChanged} />)}
    </ol>
    {!shown.length && <p className="muted">None here.</p>}
  </div>;
}
