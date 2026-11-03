import { useEffect, useId, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import { ActionButton, failureOf, outcomeOf, type Outcome } from './ActionButton';
import type { Action } from './source';

/** "Stops 2 agents" → 2; null when the note gives no count. */
export function countIn(note: string | undefined): number | null {
  const m = note?.match(/\d+/);
  return m ? Number(m[0]) : null;
}

/** The confirm question: one agent by itself, else how many. */
export function stopQuestion(n: number | null): string {
  if (n === 1) return 'Stop the agent now?';
  return n !== null ? `Stop all ${n} agents now?` : 'Stop all agents now?';
}

/** Tab and Shift+Tab stay inside `box`; Esc closes. */
function trapKeys(box: RefObject<HTMLElement | null>, close: () => void) {
  return (e: KeyboardEvent) => {
    if (e.key === 'Escape') { e.stopPropagation(); close(); return; }
    if (e.key !== 'Tab' || !box.current) return;
    const items = [...box.current.querySelectorAll<HTMLElement>('button:not(:disabled)')];
    if (items.length === 0) return;
    const [first, last] = [items[0], items[items.length - 1]];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  };
}

type ConfirmProps = { n: number | null; sending: boolean; stop(): void; close(): void };

/** The confirm step: Stop now and Cancel (focused first). Focus stays inside; Esc cancels. */
function Confirm({ n, sending, stop, close }: ConfirmProps) {
  const box = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const qid = useId();
  useEffect(() => cancel.current?.focus(), []);
  return (
    <div ref={box} className="ctl-confirm" role="alertdialog" aria-label="Emergency stop" aria-describedby={qid} aria-busy={sending} onKeyDown={trapKeys(box, close)}>
      <p id={qid} className="ctl-confirm-q">{stopQuestion(n)}</p>
      <p className="ctl-note">{n === 1 ? 'It stops at once, with every agent it started.' : 'Each stops at once, with every agent it started.'} Sessions are not kept.</p>
      <div className="ctl-row">
        <button type="button" className="btn btn-sm btn-danger danger" disabled={sending} onClick={stop}>{sending ? 'Stopping…' : 'Stop now'}</button>
        <button ref={cancel} type="button" className="btn btn-sm btn-secondary" disabled={sending} onClick={close}>Cancel</button>
      </div>
      {sending && <p className="ctl-why" role="status">Stopping…</p>}
    </div>
  );
}

/** A press outside `box` while `on` calls `close`. */
function useOutsideClose(box: RefObject<HTMLElement | null>, on: boolean, close: () => void) {
  const latest = useRef(close);
  useEffect(() => { latest.current = close; });
  useEffect(() => {
    if (!on) return;
    const down = (e: PointerEvent) => { if (!box.current?.contains(e.target as Node)) latest.current(); };
    document.addEventListener('pointerdown', down);
    return () => document.removeEventListener('pointerdown', down);
  }, [box, on]);
}

type Props = {
  action: Action;
  onOutcome(o: Outcome | null): void;
  /** Where focus goes once the stop is sent (its button is then disabled). */
  afterStop(): void;
  /** A shorter caption for its reason, when the reason is said once beside the controls. */
  shortWhy?: (why: string) => string | null;
};

/**
 * Emergency stop: disabled with its reason when nothing runs; otherwise it asks first, naming how many
 * agents it stops (counted when it opened). The question closes once the stop can no longer run (it
 * never reopens by itself), on Cancel, Esc, or a press outside it. While its own stop is being sent it
 * stays, saying "Stopping…", then closes.
 */
export function EmergencyStop({ action, onOutcome, afterStop, shortWhy }: Props) {
  const [asking, setAsking] = useState<{ n: number | null } | null>(null);
  const [sending, setSending] = useState(false);
  const box = useRef<HTMLSpanElement>(null);
  if (asking && !sending && !action.can.ok) setAsking(null);
  const button = () => box.current?.querySelector<HTMLButtonElement>('button');
  const close = () => { if (sending) return; setAsking(null); button()?.focus(); };
  useOutsideClose(box, !!asking, close);
  const stop = async () => {
    setSending(true);
    try { onOutcome(outcomeOf(await action.run())); } catch (e) { onOutcome(failureOf(e)); }
    setSending(false);
    setAsking(null);
    afterStop();
  };
  const open = () => setAsking((a) => (a ? null : { n: countIn(action.can.ok ? action.can.note : undefined) }));
  return (
    <span ref={box} className="ctl-estop">
      <ActionButton action={action} label="Emergency stop" glyph="■" className="btn-danger danger ctl-estop-btn" onOutcome={onOutcome} onPress={open} expanded={!!asking} shortWhy={shortWhy} />
      {asking && <Confirm n={asking.n} sending={sending} stop={() => void stop()} close={close} />}
    </span>
  );
}
