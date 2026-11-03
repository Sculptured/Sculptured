import { useState } from 'react';
import { Link } from 'react-router-dom';
import { questionsPath } from '../../paths';
import { nameIn, useControl } from '../context';
import { ActionError, type Approval } from '../source';

/**
 * Approvals: what agents wait on the operator for. A proposed change or a tool request is approved,
 * or rejected with a reason the agent is given. A question that needs a written answer links to
 * Needs you (the Questions page). Titles are the agents' own words.
 */

const KIND_WORDS: Record<Approval['kind'], string> = { change: '◆ change', question: '⚙ tool request', answer: '? question' };

type Phase = { kind: 'idle' } | { kind: 'rejecting'; reason: string } | { kind: 'sending' } | { kind: 'failed'; why: string };

function useDecide(id: string) {
  const { source } = useControl();
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' });
  const send = async (d: { allow: true } | { allow: false; reason: string }) => {
    setPhase({ kind: 'sending' });
    try {
      await source.decide(id, d);
      setPhase({ kind: 'idle' });
    } catch (e) {
      setPhase({ kind: 'failed', why: e instanceof ActionError ? e.message : 'Could not send the decision. Try again.' });
    }
  };
  return { phase, setPhase, send };
}

function RejectForm({ reason, set, send, cancel }: { reason: string; set(r: string): void; send(): void; cancel(): void }) {
  const empty = !reason.trim();
  return (
    <form className="ctl-reject" onSubmit={(e) => { e.preventDefault(); if (!empty) send(); }}>
      <label className="ctl-field">Reason (the agent is told)
        <input autoFocus value={reason} onChange={(e) => set(e.target.value)} onKeyDown={(e) => { if (e.key === 'Escape') cancel(); }} />
      </label>
      <button type="submit" className="btn btn-sm btn-danger danger" disabled={empty}>Reject</button>
      <button type="button" className="btn btn-sm btn-ghost" onClick={cancel}>Cancel</button>
      {empty && <span className="ctl-why">Type a reason to reject.</span>}
    </form>
  );
}

function Decide({ a }: { a: Approval }) {
  const { team } = useControl();
  const { phase, setPhase, send } = useDecide(a.id);
  if (a.kind === 'answer') return <Link className="btn btn-secondary btn-sm" to={questionsPath(team)}>Answer in Needs you</Link>;
  if (a.can && !a.can.ok) return <span className="ctl-why">{a.can.why}</span>;
  if (phase.kind === 'rejecting')
    return <RejectForm reason={phase.reason} set={(reason) => setPhase({ kind: 'rejecting', reason })} send={() => void send({ allow: false, reason: phase.reason.trim() })} cancel={() => setPhase({ kind: 'idle' })} />;
  const busy = phase.kind === 'sending';
  return (
    <span className="ctl-decide">
      <button type="button" className="btn btn-sm btn-secondary ctl-approve" disabled={busy} onClick={() => void send({ allow: true })}>Approve</button>
      <button type="button" className="btn btn-secondary btn-sm" disabled={busy} onClick={() => setPhase({ kind: 'rejecting', reason: '' })}>Reject…</button>
      {busy && <span className="ctl-why" role="status">Sending…</span>}
      {phase.kind === 'failed' && <span className="ctl-why tone-bad" role="alert">▲ {phase.why}</span>}
    </span>
  );
}

function Row({ a }: { a: Approval }) {
  const { source } = useControl();
  return (
    <li className="ctl-approval" aria-label={`${KIND_WORDS[a.kind].slice(2)} from ${nameIn(source, a.agent)}`}>
      <div className="ctl-approval-head">
        <span className={`ctl-kind is-${a.kind}`}>{KIND_WORDS[a.kind]}</span>
        <span className="ctl-who">{nameIn(source, a.agent)}</span>
        <span className="ctl-said" data-raw-evidence title="The agent's own words">{a.title}</span>
      </div>
      {a.detail && <details className="ctl-detail"><summary>Details (the agent’s words)</summary><p data-raw-evidence>{a.detail}</p></details>}
      <Decide a={a} />
    </li>
  );
}

export function ApprovalsPane() {
  const { source } = useControl();
  if (source.approvals.length === 0) return <p className="ctl-empty">✓ Nothing is waiting for you.</p>;
  return (
    <ol className="ctl-approvals" aria-label="Waiting for you">
      {source.approvals.map((a) => <Row key={a.id} a={a} />)}
    </ol>
  );
}
