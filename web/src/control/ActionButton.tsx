import { useId, useState } from 'react';
import { ActionError, type Action, type ActionReport } from './source';

/** What an action did, as plain lines: its report (with each agent it left alone, and notes), or its failure. */
export type Outcome = { tone: 'done' | 'bad'; text: string; lines: string[] };

export function outcomeOf(r: ActionReport | void, doneText?: string): Outcome | null {
  if (!r) return doneText ? { tone: 'done', text: doneText, lines: [] } : null;
  const lines = [...r.skipped.map((s) => `${s.name}: ${s.reason}`), ...(r.notes ?? []).map((n) => `${n.name}: ${n.text}`)];
  return { tone: 'done', text: r.text, lines };
}

export const failureOf = (e: unknown): Outcome => ({ tone: 'bad', text: e instanceof ActionError ? e.message : 'That did not work. Try again.', lines: [] });

/** Runs `action` once per press (a second press while it runs gets the same request), reporting how it went. */
export function useRunAction(onOutcome: (o: Outcome | null) => void, doneText?: string) {
  const [busy, setBusy] = useState(false);
  const run = async (action: Action) => {
    setBusy(true);
    try { onOutcome(outcomeOf(await action.run(), doneText)); } catch (e) { onOutcome(failureOf(e)); } finally { setBusy(false); }
  };
  return { busy, run };
}

type Props = { action: Action; label: string; glyph: string; className?: string; onOutcome(o: Outcome | null): void; doneText?: string; onPress?(): void };

/**
 * A control's button with its state in words: while it cannot run, it is disabled and says why in
 * visible text beside it. `onPress` replaces running it at once (a confirm step).
 */
export function ActionButton({ action, label, glyph, className, onOutcome, doneText, onPress }: Props) {
  const id = useId();
  const { busy, run } = useRunAction(onOutcome, doneText);
  const why = action.can.ok ? null : action.can.why;
  return (
    <span className="ctl-act">
      <button type="button" className={`btn btn-sm ${className ?? 'btn-secondary'}`} disabled={!!why || busy} aria-describedby={why ? id : undefined}
        onClick={() => (onPress ? onPress() : void run(action))}>
        <span aria-hidden="true">{glyph}</span> {label}
      </button>
      {why && <span id={id} className="ctl-why">{why}</span>}
    </span>
  );
}

/** An outcome's lines under the controls: what happened, every agent left alone and why. */
export function OutcomeNote({ o, dismiss }: { o: Outcome | null; dismiss(): void }) {
  if (!o) return null;
  return (
    <div className={`ctl-outcome${o.tone === 'bad' ? ' tone-bad' : ''}`} role={o.tone === 'bad' ? 'alert' : 'status'}>
      <p>{o.tone === 'bad' ? '▲ ' : ''}{o.text}</p>
      {o.lines.length > 0 && <ul>{o.lines.map((l) => <li key={l}>{l}</li>)}</ul>}
      <button type="button" className="btn btn-sm btn-ghost" onClick={dismiss}>Dismiss</button>
    </div>
  );
}
