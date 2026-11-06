import type { AgentLine, FeedItem } from './source';

/**
 * The master caution rules. A source tags each feed entry with the rule it raises (`FeedItem.alarm`);
 * this module turns tagged entries into alarms by the user's rules. Pure: the same input always gives
 * the same alarms, so acknowledgements (by alarm id) stay valid across updates.
 */

/** One rule: on or off, and a threshold in seconds where the rule has one. */
export type AlarmRule = { id: string; on: boolean; threshold?: number };
export type Alarm = { id: string; rule: string; at: number; agent?: string; text: string };

export const DEFAULT_RULES: AlarmRule[] = [
  { id: 'failed', on: true },
  { id: 'attention', on: true },
  { id: 'stall', on: true },
  { id: 'context-ceiling', on: true },
  { id: 'limit', on: true },
  { id: 'question-waiting', on: true, threshold: 300 },
  { id: 'part-error', on: true },
  { id: 'refused', on: true },
  { id: 'safety', on: true },
];

/** Each rule in plain words, for the Alarms pane's settings. */
export const RULE_WORDS: Record<string, string> = {
  failed: 'An agent failed',
  attention: 'An agent needs attention',
  stall: 'An agent may be stuck (suspected)',
  'context-ceiling': 'An agent reached its context limit',
  limit: 'A usage limit or the run budget was reached',
  'question-waiting': 'A question has waited longer than the threshold',
  'part-error': 'A part of the world reported an error (simulated runs)',
  refused: "A system or a gate refused an agent's call",
  safety: 'A hard safety check failed (Arena runs)',
};

/**
 * Each rule's priority (ISA-18.2 style: how soon the operator must act). High: something failed or
 * was unsafe; medium: something waits on the operator or was refused; low: worth knowing.
 */
export type Priority = 'high' | 'medium' | 'low';
const PRIORITY: Record<string, Priority> = {
  failed: 'high', safety: 'high', 'part-error': 'high',
  attention: 'medium', 'question-waiting': 'medium', refused: 'medium', stall: 'medium', limit: 'medium',
  'context-ceiling': 'low',
};
export const priorityOf = (rule: string): Priority => PRIORITY[rule] ?? 'medium';
export const PRIORITY_WORDS: Record<Priority, { mark: string; word: string; rank: number }> = {
  high: { mark: 'P1', word: 'high', rank: 0 },
  medium: { mark: 'P2', word: 'medium', rank: 1 },
  low: { mark: 'P3', word: 'low', rank: 2 },
};

/** A call's error in a system's own words reads as a refusal (a gate, a rule, a permission), not a fault. */
export const REFUSAL_WORDS = /permission|refus|denied|not allowed|not permitted|forbidden|read[- ]only|requires approval|not approved|blocked by|safety rule|user rejected/i;

const validThreshold = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v) && v >= 0;

/** Saved rules (any shape) over the defaults: unknown rules are dropped, new ones added, bad values ignored. */
export function withDefaults(saved: unknown): AlarmRule[] {
  const list = Array.isArray(saved) ? (saved as Partial<Record<string, unknown>>[]) : [];
  return DEFAULT_RULES.map((d) => {
    const s = list.find((r) => r && typeof r === 'object' && r.id === d.id);
    if (!s) return d;
    const on = typeof s.on === 'boolean' ? s.on : d.on;
    const threshold = d.threshold !== undefined && validThreshold(s.threshold) ? s.threshold : d.threshold;
    return threshold === undefined ? { id: d.id, on } : { id: d.id, on, threshold };
  });
}

/**
 * The alarms `feed` raises under `rules` at `now`, in feed order. A rule with a threshold fires once
 * its entry has waited that many seconds. An alarm's id is its rule and the condition's key, so it is
 * the same whichever entry or status reports it. An agent whose status raises an alarm now (`alarm`)
 * adds one unless the list already has it (its event is older than the list).
 */
export function alarmsFrom(feed: FeedItem[], agents: AgentLine[], rules: AlarmRule[], now: number): Alarm[] {
  const on = new Map(rules.filter((r) => r.on).map((r) => [r.id, r]));
  const out = new Map<string, Alarm>();
  for (const item of feed) {
    const rule = item.alarm ? on.get(item.alarm) : undefined;
    if (!rule) continue;
    const waited = now - item.at;
    if (rule.threshold !== undefined && waited < rule.threshold * 1000) continue;
    const id = `${rule.id}:${item.alarmKey ?? item.id}`;
    const text = rule.threshold !== undefined ? `${item.text} (waiting ${Math.floor(waited / 60_000)} min)` : item.text;
    if (!out.has(id)) out.set(id, { id, rule: rule.id, at: item.at, ...(item.agent ? { agent: item.agent } : {}), text });
  }
  for (const a of fromStatus(feed, agents, on, now)) if (!out.has(a.id)) out.set(a.id, a);
  return [...out.values()];
}

function fromStatus(feed: FeedItem[], agents: AgentLine[], on: Map<string, AlarmRule>, now: number): Alarm[] {
  return agents.flatMap((a) => {
    if (!a.alarm || !on.has(a.alarm.rule)) return [];
    // When is unknown: the time of its latest entry in the list, else now.
    const at = feed.filter((i) => i.agent === a.id).at(-1)?.at ?? now;
    return [{ id: `${a.alarm.rule}:${a.alarm.key}`, rule: a.alarm.rule, at, agent: a.id, text: `${a.name}: ${a.status.word} (its status now)` }];
  });
}
