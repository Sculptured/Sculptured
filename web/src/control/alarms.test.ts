import { describe, expect, it } from 'vitest';
import { alarmsFrom, DEFAULT_RULES, withDefaults, type AlarmRule } from './alarms';
import type { AgentLine, FeedItem } from './source';

const item = (id: string, alarm: string | undefined, at = 1_000, agent = 'a'): FeedItem => ({
  id, at, agent, kind: 'k', text: `text ${id}`, raw: {}, alarm,
});
const line = (id: string, word: string): AgentLine => ({ id, name: id.toUpperCase(), status: { glyph: '•', word, tone: 'idle' } });
const RULE_IDS = ['failed', 'attention', 'stall', 'context-ceiling', 'limit', 'question-waiting', 'part-error'];
const LATER = 10 * 60_000;

describe('alarm rules', () => {
  it('alarm_rules_each_fire_and_can_be_switched_off', () => {
    expect(DEFAULT_RULES.map((r) => r.id)).toEqual(RULE_IDS);
    expect(DEFAULT_RULES.every((r) => r.on)).toBe(true);
    const feed = [item('quiet', undefined), ...RULE_IDS.map((r) => item(`e-${r}`, r))];
    const all = alarmsFrom(feed, [], DEFAULT_RULES, LATER);
    expect(all.map((a) => a.rule)).toEqual(RULE_IDS);
    expect(all[0]).toEqual({ id: 'failed:e-failed', rule: 'failed', at: 1_000, agent: 'a', text: 'text e-failed' });
    for (const off of RULE_IDS) {
      const rules = DEFAULT_RULES.map((r) => (r.id === off ? { ...r, on: false } : r));
      expect(alarmsFrom(feed, [], rules, LATER).map((a) => a.rule)).toEqual(RULE_IDS.filter((r) => r !== off));
    }
  });

  it('question_waiting_threshold', () => {
    const feed = [item('q1', 'question-waiting', 0)];
    expect(DEFAULT_RULES.find((r) => r.id === 'question-waiting')?.threshold).toBe(300);
    expect(alarmsFrom(feed, [], DEFAULT_RULES, 299_999)).toEqual([]);
    const fired = alarmsFrom(feed, [], DEFAULT_RULES, 300_000);
    expect(fired).toHaveLength(1);
    expect(fired[0].text).toBe('text q1 (waiting 5 min)');
    const sooner: AlarmRule[] = DEFAULT_RULES.map((r) => (r.id === 'question-waiting' ? { ...r, threshold: 60 } : r));
    expect(alarmsFrom(feed, [], sooner, 60_000)).toHaveLength(1);
    expect(alarmsFrom(feed, [], sooner, 59_000)).toEqual([]);
  });

  it('an agent failed or needing attention now raises an alarm when its event is not in the list', () => {
    const agents = [line('a', 'failed'), line('b', 'needs attention'), line('c', 'running')];
    const fromState = alarmsFrom([item('x', undefined, 5, 'b')], agents, DEFAULT_RULES, 9);
    expect(fromState).toEqual([
      { id: 'failed:now:a', rule: 'failed', at: 9, agent: 'a', text: 'A: failed (its status now)' },
      { id: 'attention:now:b', rule: 'attention', at: 5, agent: 'b', text: 'B: needs attention (its status now)' },
    ]);
    // The event's own alarm is enough: no second one from the status.
    expect(alarmsFrom([item('f', 'failed', 3, 'a')], [line('a', 'failed')], DEFAULT_RULES, 9).map((a) => a.id)).toEqual(['failed:f']);
  });

  it('saved rules keep the user choices, gain new rules and drop unknown ones', () => {
    const merged = withDefaults([{ id: 'stall', on: false }, { id: 'question-waiting', on: true, threshold: 30 }, { id: 'gone', on: true }]);
    expect(merged.map((r) => r.id)).toEqual(RULE_IDS);
    expect(merged.find((r) => r.id === 'stall')?.on).toBe(false);
    expect(merged.find((r) => r.id === 'question-waiting')?.threshold).toBe(30);
    expect(withDefaults('nonsense')).toEqual(DEFAULT_RULES);
    expect(withDefaults([{ id: 'question-waiting', on: 'yes', threshold: -4 }])).toEqual(DEFAULT_RULES);
  });
});
