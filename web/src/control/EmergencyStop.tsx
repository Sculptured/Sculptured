import { useEffect, useRef, useState } from 'react';
import { ActionButton, failureOf, outcomeOf, type Outcome } from './ActionButton';
import type { Action } from './source';

/** "Stops 2 agents" → 2; null when the note gives no count. */
export function countIn(note: string | undefined): number | null {
  const m = note?.match(/\d+/);
  return m ? Number(m[0]) : null;
}

type ConfirmProps = { action: Action; close(): void; onOutcome(o: Outcome | null): void };

/** The confirm step: "Stop all N agents now?", Stop now and Cancel (focused first; Esc cancels). Stop now sends once. */
function Confirm({ action, close, onOutcome }: ConfirmProps) {
  const cancel = useRef<HTMLButtonElement>(null);
  const [sending, setSending] = useState(false);
  useEffect(() => cancel.current?.focus(), []);
  const n = countIn(action.can.ok ? action.can.note : undefined);
  const stop = async () => {
    setSending(true);
    try { onOutcome(outcomeOf(await action.run())); } catch (e) { onOutcome(failureOf(e)); }
    close();
  };
  return (
    <div className="ctl-confirm" role="alertdialog" aria-label="Emergency stop" aria-describedby="ctl-estop-q"
      onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } }}>
      <p id="ctl-estop-q" className="ctl-confirm-q">{n !== null ? `Stop all ${n} agents now?` : 'Stop all agents now?'}</p>
      <p className="ctl-note">Each stops at once, with every agent it started. Their sessions are not kept.</p>
      <div className="ctl-row">
        <button type="button" className="btn btn-sm btn-danger danger" disabled={sending} onClick={() => void stop()}>{sending ? 'Stopping…' : 'Stop now'}</button>
        <button ref={cancel} type="button" className="btn btn-secondary btn-sm" disabled={sending} onClick={close}>Cancel</button>
      </div>
    </div>
  );
}

/** Emergency stop: disabled with its reason when nothing runs; otherwise it asks first. */
export function EmergencyStop({ action, onOutcome }: { action: Action; onOutcome(o: Outcome | null): void }) {
  const [asking, setAsking] = useState(false);
  const opener = useRef<HTMLSpanElement>(null);
  const close = () => { setAsking(false); opener.current?.querySelector('button')?.focus(); };
  return (
    <span ref={opener} className="ctl-estop">
      <ActionButton action={action} label="Emergency stop" glyph="■" className="btn-danger danger ctl-estop-btn" onOutcome={onOutcome} onPress={() => setAsking((a) => !a)} />
      {asking && action.can.ok && <Confirm action={action} close={close} onOutcome={onOutcome} />}
    </span>
  );
}
