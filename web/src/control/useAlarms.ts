import { useCallback, useEffect, useMemo, useState } from 'react';
import { alarmsFrom, withDefaults, type Alarm, type AlarmRule } from './alarms';
import type { ControlSource } from './source';

/**
 * The master caution for one team: its alarms under the user's rules, and which of them the user
 * acknowledged. Both are kept per team in localStorage (acknowledgements under
 * `sculptured.alarms.ack.<team>`), and other windows (a popped-out pane) see changes at once.
 */

export const ackKey = (team: string) => `sculptured.alarms.ack.${team}`;
export const rulesKey = (team: string) => `sculptured.alarms.rules.${team}`;
export const shelfKey = (team: string) => `sculptured.alarms.shelf.${team}`;
/** How long Shelve takes an alarm out of the master caution. */
export const SHELVE_MS = 3_600_000;
/** Acknowledgements kept per team; the oldest go first. */
const MAX_ACKS = 500;

function read(key: string): unknown {
  try { return JSON.parse(localStorage.getItem(key) ?? 'null'); } catch { return null; }
}
function write(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* Kept for this session only. */ }
}

const parseAcks = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);
/** Shelved alarms: id → until when (ms). Bad entries are dropped. */
const parseShelf = (v: unknown): Record<string, number> => Object.fromEntries(
  Object.entries(v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {}).filter((e): e is [string, number] => typeof e[1] === 'number'),
);

/** A value in localStorage under `key`, read with `parse` (module level, so stable), followed across windows. */
function useStored<T>(key: string, parse: (v: unknown) => T) {
  const [value, setValue] = useState(() => parse(read(key)));
  const [shownKey, setShownKey] = useState(key);
  if (shownKey !== key) { setShownKey(key); setValue(parse(read(key))); }
  useEffect(() => {
    const on = (e: StorageEvent) => { if (e.key === key) setValue(parse(read(key))); };
    window.addEventListener('storage', on);
    return () => window.removeEventListener('storage', on);
  }, [key, parse]);
  const update = useCallback((fn: (v: T) => T) => setValue((prev) => { const next = fn(prev); write(key, next); return next; }), [key]);
  return [value, update] as const;
}

export type AlarmsApi = {
  /** Every alarm the rules raise now, oldest first. */
  list: Alarm[];
  acked: Set<string>;
  /** How many are neither acknowledged nor shelved: the master caution's count. */
  unacked: number;
  ack(id: string): void;
  ackAll(): void;
  /** Shelved alarms and until when; a shelved alarm stays listed but leaves the master caution. */
  shelved: Map<string, number>;
  shelve(id: string): void;
  unshelve(id: string): void;
  rules: AlarmRule[];
  setRules(rules: AlarmRule[]): void;
};

export function useAlarms(team: string, source: Pick<ControlSource, 'feed' | 'agents'>, now: number): AlarmsApi {
  const [acks, setAcks] = useStored(ackKey(team), parseAcks);
  const [rules, setStoredRules] = useStored(rulesKey(team), withDefaults);
  const [shelf, setShelf] = useStored(shelfKey(team), parseShelf);
  const list = useMemo(() => alarmsFrom(source.feed, source.agents, rules, now), [source.feed, source.agents, rules, now]);
  const acked = useMemo(() => new Set(acks), [acks]);
  const shelved = useMemo(() => new Map(Object.entries(shelf).filter(([, until]) => until > now)), [shelf, now]);
  const add = (ids: string[]) => setAcks((cur) => [...cur, ...ids.filter((id) => !cur.includes(id))].slice(-MAX_ACKS));
  return {
    list,
    acked,
    unacked: list.filter((a) => !acked.has(a.id) && !shelved.has(a.id)).length,
    ack: (id) => add([id]),
    ackAll: () => add(list.filter((a) => !shelved.has(a.id)).map((a) => a.id)),
    shelved,
    shelve: (id) => setShelf((cur) => ({ ...cur, [id]: Date.now() + SHELVE_MS })),
    unshelve: (id) => setShelf((cur) => Object.fromEntries(Object.entries(cur).filter(([k]) => k !== id))),
    rules,
    setRules: (next) => setStoredRules(() => next),
  };
}

/** A clock for thresholds and "how long ago", ticking every `ms`. */
export function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}
