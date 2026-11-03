import { useId, useState, type ReactNode, type Ref } from 'react';
import { ActionError, type Action, type ActionReport } from './source';

/** One line under an outcome: an agent and what to know about it. `raw`: the server's own words (evidence). */
export type OutcomeLine = { name: string; text: string; raw: boolean };

/**
 * What an action did, as plain lines: its report (with each agent it left alone, and notes), why it
 * did nothing (`refused`, never shown as a success), or its failure.
 */
export type Outcome = { tone: 'done' | 'refused' | 'bad'; text: string; lines: OutcomeLine[] };

export function outcomeOf(r: ActionReport | void, doneText?: string): Outcome | null {
  if (!r) return doneText ? { tone: 'done', text: doneText, lines: [] } : null;
  const lines = [
    ...r.skipped.map((s) => ({ name: s.name, text: s.reason, raw: !!s.raw })),
    ...(r.notes ?? []).map((n) => ({ name: n.name, text: n.text, raw: true })),
  ];
  return { tone: r.refused ? 'refused' : 'done', text: r.text, lines };
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

type Props = {
  action: Action;
  label: string;
  glyph: string;
  className?: string;
  onOutcome(o: Outcome | null): void;
  doneText?: string;
  /** Replaces running it at once (a confirm step). */
  onPress?(): void;
  /** Whether its confirm step is open. */
  expanded?: boolean;
  /** A shorter caption for its reason, when the full reason is plain elsewhere (it stays the tooltip). */
  shortWhy?: (why: string) => string | null;
  /** Beside the reason while it cannot run: where to fix it. */
  fix?: ReactNode;
};

/** A control's button with its state in words: while it cannot run, it is disabled and says why in visible text beside it. */
export function ActionButton({ action, label, glyph, className, onOutcome, doneText, onPress, expanded, shortWhy, fix }: Props) {
  const id = useId();
  const { busy, run } = useRunAction(onOutcome, doneText);
  const why = action.can.ok ? null : action.can.why;
  const caption = why ? (shortWhy?.(why) ?? why) : null;
  return (
    <span className="ctl-act">
      <button type="button" className={`btn btn-sm ${className ?? 'btn-secondary'}`} disabled={!!why || busy} aria-describedby={why ? id : undefined}
        aria-expanded={expanded} title={caption !== why ? (why ?? undefined) : undefined} onClick={() => (onPress ? onPress() : void run(action))}>
        <span aria-hidden="true">{glyph}</span> {label}
      </button>
      {why && <span id={id} className="ctl-why">{caption}{caption !== why && <span className="sr-only">: {why}</span>}</span>}
      {why && fix}
    </span>
  );
}

/** An outcome's lines under the controls: what happened, every agent left alone and why. */
const MARK = { done: '', refused: '◇ ', bad: '▲ ' } as const;
const TONE = { done: '', refused: ' tone-warn', bad: ' tone-bad' } as const;

/** `ref` (focusable) takes focus after an action whose own button is gone or disabled. */
export function OutcomeNote({ o, dismiss, ref }: { o: Outcome | null; dismiss(): void; ref?: Ref<HTMLDivElement> }) {
  if (!o) return null;
  return (
    <div ref={ref} tabIndex={-1} className={`ctl-outcome${TONE[o.tone]}`} role={o.tone === 'bad' ? 'alert' : 'status'}>
      <p>{MARK[o.tone]}{o.text}</p>
      {o.lines.length > 0 && (
        <ul aria-label="Each agent">
          {o.lines.map((l, i) => <li key={i}>{l.name}: {l.raw ? <span data-raw-evidence>{l.text}</span> : l.text}</li>)}
        </ul>
      )}
      <button type="button" className="btn btn-sm btn-ghost" onClick={dismiss}>Dismiss</button>
    </div>
  );
}
