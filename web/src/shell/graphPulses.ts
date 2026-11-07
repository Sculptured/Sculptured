import { useEffect, useMemo, useRef, useState } from 'react';
import type { EventRecord } from '../generated/EventRecord';

/** Key of the edge a handoff travels along. */
export const pulseKey = (from: string, to: string) => `${from}\u2192${to}`;

/** How long an edge pulses after a handoff. */
export const PULSE_MS = 1800;

type Handed = { seq: number; to: string; from: string[] };

const handedOver = (feed: Record<string, EventRecord[]>): Handed[] =>
  Object.values(feed).flatMap((list) =>
    list.flatMap((e) => (e.body.type === 'handed-over' && e.subject.kind === 'seat' ? [{ seq: e.seq, to: e.subject.id, from: e.body.from.map((f) => f.seat) }] : [])),
  );

/** One more (or one fewer) pulse on each key; a key with none left is dropped. */
function adjust(m: Map<string, number>, keys: string[], by: 1 | -1) {
  const next = new Map(m);
  for (const k of keys) {
    const n = (next.get(k) ?? (by === 1 ? 0 : 1)) + by;
    if (n <= 0) next.delete(k);
    else next.set(k, n);
  }
  return next;
}

/**
 * Edges that carried a handoff in the last `PULSE_MS`: each live `handed-over` event (on the
 * receiving seat) pulses the edge from every upstream seat it names. Events already in the
 * feed when the graph opens do not pulse.
 */
export function useHandoffPulses(feed: Record<string, EventRecord[]>, ms = PULSE_MS): Set<string> {
  const seen = useRef<Set<number> | null>(null);
  // Each pulse ends on its own timer. The timers live here, not in the effect's cleanup:
  // the feed changes on every event, and a cleanup would cancel a pulse's end for good.
  const timers = useRef<Set<ReturnType<typeof setTimeout>>>(new Set());
  const [active, setActive] = useState<Map<string, number>>(() => new Map());
  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const t of pending) clearTimeout(t);
      pending.clear();
    };
  }, []);
  const handed = useMemo(() => handedOver(feed), [feed]);
  useEffect(() => {
    if (seen.current === null) {
      seen.current = new Set(handed.map((h) => h.seq));
      return;
    }
    const fresh = handed.filter((h) => !seen.current!.has(h.seq));
    if (fresh.length === 0) return;
    for (const h of fresh) seen.current.add(h.seq);
    const keys = fresh.flatMap((h) => h.from.map((f) => pulseKey(f, h.to)));
    setActive((m) => adjust(m, keys, 1));
    const t = setTimeout(() => {
      timers.current.delete(t);
      setActive((m) => adjust(m, keys, -1));
    }, ms);
    timers.current.add(t);
  }, [handed, ms]);
  return useMemo(() => new Set(active.keys()), [active]);
}
