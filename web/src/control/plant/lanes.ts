import type { PlantAction } from './model';

/**
 * The action timeline's lanes (a sequence-of-events recorder): one lane per agent, each action a mark
 * at its time. Marks that would overlap go on the next row of the lane (up to `rows`; past that they
 * share the last row and the list view has them all). Pure.
 */

export type Range = { start: number; end: number };
export type Placed = { action: PlantAction; left: number; row: number };
export type Lane = { agent: string; marks: Placed[]; rows: number };

/** From the first action to `now` (or the last action, if later); null with no actions. */
export function actionRange(actions: PlantAction[], now: number, windowMs?: number): Range | null {
  if (actions.length === 0) return null;
  const end = Math.max(now, ...actions.map((a) => a.at + (a.ms ?? 0)));
  const start = windowMs ? end - windowMs : Math.min(...actions.map((a) => a.at));
  return { start, end: end > start ? end : start + 1000 };
}

const pct = (r: Range, at: number) => Math.min(100, Math.max(0, ((at - r.start) / (r.end - r.start)) * 100));

/** Places one lane's actions, oldest first; `gap` is the least distance (in % of the width) between two marks on a row. */
export function placeLane(actions: PlantAction[], r: Range, gap = 4, rows = 3): Placed[] {
  const lastOnRow: number[] = [];
  return [...actions].filter((a) => a.at >= r.start && a.at <= r.end).sort((a, b) => a.at - b.at).map((action) => {
    const left = pct(r, action.at);
    let row = lastOnRow.findIndex((x) => left - x >= gap);
    if (row === -1) row = lastOnRow.length < rows ? lastOnRow.length : rows - 1;
    lastOnRow[row] = left;
    return { action, left, row };
  });
}

/** A lane per agent in `agents` order, then any other agent that acted. */
export function lanesOf(actions: PlantAction[], agents: string[], r: Range, hidden: Set<string> = new Set()): Lane[] {
  const ids = [...agents, ...new Set(actions.map((a) => a.agent).filter((a) => !agents.includes(a)))];
  return ids.filter((id) => !hidden.has(id)).map((agent) => {
    const marks = placeLane(actions.filter((a) => a.agent === agent), r);
    return { agent, marks, rows: Math.max(1, ...marks.map((m) => m.row + 1)) };
  });
}

/** Actions matching a filter: one system, writes only, abnormal only. */
export type ActionFilter = { system?: string; writesOnly?: boolean; abnormalOnly?: boolean };

export function filterActions(actions: PlantAction[], f: ActionFilter): PlantAction[] {
  return actions.filter((a) => (!f.system || a.system === f.system)
    && (!f.writesOnly || a.effect === 'write')
    && (!f.abnormalOnly || a.outcome === 'refused' || a.outcome === 'error'));
}
