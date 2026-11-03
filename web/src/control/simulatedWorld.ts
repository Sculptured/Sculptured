import { arena } from '../tests/api';
import { MAX_ROW_KEYS } from '../tests/model';
import type { BranchDetail, Event } from '../tests/types';

/** The world of a simulated run at a step: which save holds it, and the rows it had touched by then. */

/** The latest save at or before `cursor`; null when nothing was saved by then. */
export function saveAt(saves: number[], cursor: number): number | null {
  let best: number | null = null;
  for (const s of saves) if (s <= cursor && (best === null || s > best)) best = s;
  return best;
}

const keysOf = (v: unknown): string[] => (Array.isArray(v) ? v.filter((k): k is string => typeof k === 'string' && !k.endsWith('/*')) : []);

type Touch = { part: string; keys: string[] };

/** What each tool call up to `upTo` touched, and in which part ('' for runs recorded before parts). */
function touches(events: Event[], upTo: number): Touch[] {
  return events
    .filter((e) => e.kind === 'tool_call' && e.seq <= upTo)
    .sort((a, b) => a.seq - b.seq)
    .flatMap((e) => {
      const p = (e.payload ?? {}) as Record<string, unknown>;
      if (p.error != null) return [];
      return [{ part: typeof p.part === 'string' ? p.part : '', keys: [...keysOf(p.writes), ...keysOf(p.reads)] }];
    });
}

/**
 * The keys to read at step `upTo`, by part: `key` alone (in the part that last touched it), or every
 * key touched by then, newest first, at most `MAX_ROW_KEYS` in all.
 */
export function keysByPart(events: Event[], upTo: number, key?: string): Map<string, string[]> {
  const all = touches(events, upTo).reverse();
  if (key) return new Map([[all.find((t) => t.keys.includes(key))?.part ?? '', [key]]]);
  const out = new Map<string, string[]>();
  const seen = new Set<string>();
  for (const t of all) {
    for (const k of t.keys) {
      if (seen.has(k) || seen.size >= MAX_ROW_KEYS) continue;
      seen.add(k);
      out.set(t.part, [...(out.get(t.part) ?? []), k]);
    }
  }
  return out;
}

/** The rows at step `save` (a step after which the world was saved): `key`, or every key touched by then. */
export async function rowsAt(b: BranchDetail, save: number, key?: string): Promise<Record<string, unknown>> {
  const groups = [...keysByPart(b.events, save, key)];
  const answers = await Promise.all(groups.map(([part, keys]) => arena.rows(b.id, save, keys, part || undefined)));
  return Object.assign({}, ...answers.map((a) => a.rows)) as Record<string, unknown>;
}
