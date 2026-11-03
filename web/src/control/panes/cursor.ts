import { useCallback, useEffect, useRef } from 'react';
import type { Step } from '../../tests/model';
import type { BranchDetail } from '../../tests/types';
import { useSelection, type Selection } from '../../workspace/selection';
import type { SimulatedTimeline } from '../source';

/**
 * The simulated timeline's cursor and the workspace selection kept together: moving the cursor
 * selects that step's moment and agent (so the Status feed and popped-out windows follow), and
 * picking a moment elsewhere moves the cursor to the step recorded then.
 */

/** When a step was recorded (ms since 1970) and by which agent. */
export function momentOf(b: BranchDetail, seq: number): { time: number; agent?: string } | undefined {
  const e = b.events.find((x) => x.seq === seq);
  if (!e) return undefined;
  const t = Date.parse(e.at);
  return { time: Number.isNaN(t) ? 0 : t, ...(e.agent ? { agent: e.agent } : {}) };
}

/** The step a selected moment names: the one that agent recorded then, else the latest at or before it. */
export function stepFor(b: BranchDetail, steps: Step[], sel: Selection): number | undefined {
  if (sel.time === undefined) return undefined;
  let best: number | undefined;
  for (const s of steps) {
    const m = momentOf(b, s.seq);
    if (!m || m.time > sel.time) continue;
    if (m.time === sel.time && sel.agent !== undefined && m.agent === sel.agent) return s.seq;
    best = s.seq;
  }
  return best;
}

/** The step `by` steps from `cursor` in order (clamped); Home and End are the first and last. */
export function stepFrom(steps: Step[], cursor: number, by: number | 'first' | 'last'): number | undefined {
  if (steps.length === 0) return undefined;
  if (by === 'first') return steps[0].seq;
  if (by === 'last') return steps[steps.length - 1].seq;
  const i = steps.findIndex((s) => s.seq === cursor);
  const from = i < 0 ? (by > 0 ? -1 : steps.length) : i;
  return steps[Math.min(steps.length - 1, Math.max(0, from + by))].seq;
}

/**
 * `move(seq)`: puts the cursor on a step and selects its moment. While this pane follows the
 * selection, a moment picked elsewhere moves the cursor (once per new selection, so a cursor just
 * moved here is never pulled back by an older selection).
 */
export function useCursor(tl: SimulatedTimeline, b: BranchDetail) {
  const { selection, select } = useSelection();
  const seen = useRef(selection);
  useEffect(() => {
    if (seen.current === selection) return;
    seen.current = selection;
    const here = momentOf(b, tl.cursor);
    if (here && here.time === selection.time && (selection.agent === undefined || here.agent === selection.agent)) return;
    const seq = stepFor(b, tl.steps, selection);
    if (seq !== undefined && seq !== tl.cursor) tl.setCursor(seq);
  }, [selection, b, tl]);
  const move = useCallback((seq: number) => {
    tl.setCursor(seq);
    const m = momentOf(b, seq);
    if (m) select({ time: m.time, agent: m.agent });
  }, [tl, b, select]);
  return move;
}
