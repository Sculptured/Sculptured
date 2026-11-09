import { useEffect, useId, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import { ActionButton, failureOf, outcomeOf, type Outcome } from './ActionButton';
import type { Action } from './source';

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

/** What the confirm step asks, and what stopping does. */
export type StopAsk = { question: string; note: string };

type ConfirmProps = { title: string; ask: StopAsk; sending: boolean; stop(): void; close(): void };

/** The confirm step: Stop now and Cancel (focused first). Focus stays inside; Esc cancels. */
function Confirm({ title, ask, sending, stop, close }: ConfirmProps) {
  const box = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const qid = useId();
  useEffect(() => cancel.current?.focus(), []);
  return (
    <div ref={box} className="ctl-confirm" role="alertdialog" aria-label={title} aria-describedby={qid} aria-busy={sending} onKeyDown={trapKeys(box, close)}>
      <p id={qid} className="ctl-confirm-q">{ask.question}</p>
      <p className="ctl-note">{ask.note}</p>
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
  /** The button's word, and the confirm step's name. */
  label: string;
  /** What to ask, read when the confirm step opens. */
  ask(): StopAsk;
  onOutcome(o: Outcome | null): void;
  /** Where focus goes once the stop is sent (its button is then disabled). */
  afterStop?(): void;
  /** Its reason is said once beside the controls. */
  quiet?: boolean;
  className?: string;
  /** What to say once a stop that reports nothing was sent. */
  doneText?: string;
};

/**
 * A stop that asks first: disabled with its reason when it cannot run; otherwise its confirm step says
 * what it stops (read when it opened). The question closes once the stop can no longer run (it never
 * reopens by itself), on Cancel, Esc, or a press outside it. While its own stop is being sent it stays,
 * saying "Stopping…", then closes.
 */
export function ConfirmedStop({ action, label, ask, onOutcome, afterStop, quiet, className = '', doneText }: Props) {
  const [asking, setAsking] = useState<StopAsk | null>(null);
  const [sending, setSending] = useState(false);
  const box = useRef<HTMLSpanElement>(null);
  if (asking && !sending && !action.can.ok) setAsking(null);
  const button = () => box.current?.querySelector<HTMLButtonElement>('button');
  const close = () => { if (sending) return; setAsking(null); button()?.focus(); };
  useOutsideClose(box, !!asking, close);
  const stop = async () => {
    setSending(true);
    try { onOutcome(outcomeOf(await action.run(), doneText)); } catch (e) { onOutcome(failureOf(e)); }
    setSending(false);
    setAsking(null);
    (afterStop ?? (() => button()?.focus()))();
  };
  const open = () => setAsking((a) => (a ? null : ask()));
  return (
    <span ref={box} className="ctl-estop">
      <ActionButton action={action} label={label} glyph="■" className={`btn-danger danger ${className}`} onOutcome={onOutcome} onPress={open} expanded={!!asking} quiet={quiet} />
      {asking && <Confirm title={label} ask={asking} sending={sending} stop={() => void stop()} close={close} />}
    </span>
  );
}

/** Emergency stop: every running agent at once, asking first how many (counted when it opened). */
export function EmergencyStop(p: Pick<Props, 'action' | 'onOutcome' | 'afterStop' | 'quiet'>) {
  const ask = (): StopAsk => {
    const n = p.action.can.ok ? (p.action.can.count ?? null) : null;
    const each = n === 1 ? 'It stops at once, with every agent it started.' : 'Each stops at once, with every agent it started.';
    return { question: stopQuestion(n), note: `${each} Sessions are not kept.` };
  };
  return <ConfirmedStop {...p} label="Emergency stop" ask={ask} className="ctl-estop-btn" />;
}
