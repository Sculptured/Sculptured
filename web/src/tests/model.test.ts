import { describe, expect, it } from 'vitest';
import { branchNames, bySteps, changedFields, diffHunks, inOrder, jsonDiff, layout, rowKeys, stepBefore, summarize, ticks } from './model';
import type { Event, EventKind, Team } from './types';

const team: Team = {
  id: 't', name: 't',
  agents: [
    { id: 'triage', name: 'Triage', kind: 'claude_code', model: null, prompt: '' },
    { id: 'resolver', name: 'Resolver', kind: 'claude_code', model: null, prompt: '' },
  ],
};
const at = (s: number) => new Date(Date.parse('2026-01-01T00:00:00Z') + s * 1000).toISOString();
const ev = (seq: number, t: number, agent: string | null, kind: EventKind, payload: unknown = {}): Event =>
  ({ branch_id: 'b', seq, at: at(t), agent, kind, payload });

const events: Event[] = [
  ev(0, 0, null, 'run_start'),
  ev(1, 0, 'triage', 'agent_start'),
  ev(2, 3, 'triage', 'tool_call', { name: 'get_order', writes: [], reads: ['orders/1'] }),
  ev(3, 5, 'triage', 'handoff', { note: 'x' }),
  ev(4, 5, 'resolver', 'agent_start'),
  ev(5, 8, 'resolver', 'model_call'),
  ev(6, 10, 'resolver', 'tool_call', { name: 'cancel_order', writes: ['orders/1'], reads: [], snapshot: 's1' }),
  ev(7, 11, 'resolver', 'tool_call', { name: 'cancel_order', writes: ['orders/1'], error: 'already cancelled' }),
  ev(8, 12, null, 'check', { passed: true }),
];

describe('layout', () => {
  it('puts each agent in its own lane in team order, the check last', () => {
    const l = layout(events, team);
    expect(l.lanes.map((x) => x.agent)).toEqual(['triage', 'resolver', 'check']);
    expect(l.lanes[0].steps.map((s) => s.seq)).toEqual([1, 2, 3]);
    expect(l.lanes[1].steps.map((s) => s.seq)).toEqual([4, 5, 6, 7]);
  });

  it('spans each step from the previous one in its lane to when it was recorded', () => {
    const l = layout(events, team);
    const s = (n: number) => inOrder(l).find((x) => x.seq === n)!;
    expect([s(2).from, s(2).to]).toEqual([0, 3000]);
    expect([s(5).from, s(5).to]).toEqual([5000, 8000]);
    expect([s(8).from, s(8).to]).toEqual([11000, 12000]);
    expect(l.span).toBe(12000);
  });

  it('tells reads, writes, failed calls and saved databases apart', () => {
    const s = (n: number) => inOrder(layout(events, team)).find((x) => x.seq === n)!;
    expect(s(2)).toMatchObject({ kind: 'read', label: 'get_order', snapshot: false, failed: false });
    expect(s(6)).toMatchObject({ kind: 'write', snapshot: true, failed: false });
    // A refused write changed nothing: it reads as a failed call, not a write.
    expect(s(7)).toMatchObject({ kind: 'read', failed: true });
    expect(s(8)).toMatchObject({ kind: 'check', label: 'Check passed' });
  });

  it('marks steps before the fork as reused and places the fork line', () => {
    const l = layout(events, team, 5);
    expect(inOrder(l).filter((s) => s.inherited).map((s) => s.seq)).toEqual([1, 2, 3, 4]);
    expect(l.forkAt).toBe(8000);
    expect(layout(events, team).forkAt).toBeNull();
  });

  it('survives a branch with no steps yet', () => {
    const l = layout([ev(0, 0, null, 'run_start')], team);
    expect(l.span).toBe(1);
    expect(inOrder(l)).toEqual([]);
  });
});

describe('bySteps', () => {
  it('gives every step one equal slot in order and moves the fork line to the first own step', () => {
    const l = bySteps(layout(events, team, 5));
    expect(inOrder(l).map((s) => [s.seq, s.from, s.to])).toEqual([[1, 0, 1], [2, 1, 2], [3, 2, 3], [4, 3, 4], [5, 4, 5], [6, 5, 6], [7, 6, 7], [8, 7, 8]]);
    expect([l.span, l.forkAt]).toEqual([8, 4]);
    expect(bySteps(layout(events, team)).forkAt).toBeNull();
  });
});

describe('ticks', () => {
  it('gives a handful of round marks', () => {
    expect(ticks(12000).map((t) => t[1])).toEqual(['0s', '2s', '4s', '6s', '8s', '10s', '12s']);
    expect(ticks(40000).map((t) => t[1])).toEqual(['0s', '10s', '20s', '30s', '40s']);
    expect(ticks(130000).length).toBeLessThanOrEqual(7);
    expect(ticks(130000).at(-1)![1]).toBe('2m');
  });
});

const end = (seq: number, p: object) => ev(seq, 0, 'triage', 'agent_end', p);
const br = { start_seq: null, created_at: '2026-01-01T00:00:00Z', ended_at: '2026-01-01T00:00:10Z', status: 'passed' as const };

describe('summarize', () => {
  it('sums the agents, keeps a single cost source, and counts rows and steps', () => {
    const s = summarize(br, [...events, end(9, { tokens: 10, cost_usd: 0.5, cost_source: 'openrouter' }), end(10, { tokens: 5, cost_usd: 0.25, cost_source: 'openrouter' })]);
    expect(s).toMatchObject({ seconds: 10, tokens: 15, cost_usd: 0.75, cost_source: 'openrouter', live: false, rows_written: 1, steps: 5 });
  });
  it('has no dollar figure when any agent lacks one, or none has finished', () => {
    expect(summarize(br, [end(2, { tokens: 1, cost_usd: null }), end(3, { tokens: 1, cost_usd: 1, cost_source: 'openrouter' })]).cost_usd).toBeNull();
    expect(summarize(br, []).cost_usd).toBeNull();
    expect(summarize(br, [end(2, { tokens: 1, cost_usd: 1, cost_source: 'openrouter' }), end(3, { tokens: 1, cost_usd: 1, cost_source: 'api_equivalent_estimate' })]).cost_source).toBe('mixed');
  });
  it('a rerun counts only its own steps; a running one counts to now', () => {
    const s = summarize({ ...br, start_seq: 4, ended_at: null, status: 'running' },
      [end(2, { tokens: 100, cost_usd: 9, cost_source: 'openrouter' }), end(6, { tokens: 7, cost_usd: 1, cost_source: 'openrouter' })],
      Date.parse('2026-01-01T00:00:03Z'));
    expect(s).toMatchObject({ tokens: 7, cost_usd: 1, seconds: 3, live: true });
  });
});

describe('branchNames', () => {
  it('calls the first branch Original and the rest Rerun N by age', () => {
    const n = branchNames([
      { id: 'c', parent_id: 'a', created_at: '2026-01-03' },
      { id: 'a', parent_id: null, created_at: '2026-01-01' },
      { id: 'b', parent_id: 'a', created_at: '2026-01-02' },
    ]);
    expect([n.get('a'), n.get('b'), n.get('c')]).toEqual(['Original', 'Rerun 1', 'Rerun 2']);
  });
});

describe('changedFields', () => {
  const agents = team.agents.map((a) => ({ ...a, model: 'sonnet', prompt: 'p' }));
  it('ignores whitespace prompt edits and unchanged values', () => {
    expect(changedFields(agents, { triage: { prompt: ' p ' }, resolver: { model: 'sonnet', kind: 'claude_code' } })).toEqual({});
  });
  it('a cleared model that was set is an explicit clear; blank on an unset model is no change', () => {
    expect(changedFields(agents, { triage: { model: '  ' } })).toEqual({ triage: { model: '' } });
    expect(changedFields(agents.map((a) => ({ ...a, model: null })), { triage: { model: '' } })).toEqual({});
  });
  it('switching mode and back, with the model edit dropped, changes nothing', () => {
    expect(changedFields(agents, { triage: { kind: 'claude_code', model: undefined } })).toEqual({});
  });
  it('a mode change with a blank model sends just the mode', () => {
    expect(changedFields(agents, { triage: { kind: 'codex', model: '' } })).toEqual({ triage: { kind: 'codex' } });
    expect(changedFields(agents, { triage: { kind: 'codex', model: 'gpt-x' } })).toEqual({ triage: { kind: 'codex', model: 'gpt-x' } });
  });
});

describe('rows', () => {
  it('drops whole-table reads, dedups with writes first, and caps at 50', () => {
    expect(rowKeys(['orders/#W1'], ['orders/*', 'users/u1', 'orders/#W1'])).toEqual({ keys: ['orders/#W1', 'users/u1'], more: 0 });
    const r = rowKeys([], Array.from({ length: 53 }, (_, i) => `products/p${i}`));
    expect([r.keys.length, r.more]).toEqual([50, 3]);
  });
  it('reads "before" from the step just before, on this branch', () => {
    expect(stepBefore(events, 6)).toBe(5);
    expect(stepBefore(events, 0)).toBeNull();
  });
});

describe('diffs', () => {
  it('marks changed lines and absent sides', () => {
    const d = jsonDiff({ a: 1, b: 2 }, { a: 1, b: 3 });
    expect(d.filter((l) => l.op !== ' ').map((l) => `${l.op}${l.text.trim()}`)).toEqual(['-"b": 2', '+"b": 3']);
    expect(jsonDiff(undefined, { a: 1 })[0]).toEqual({ op: '-', text: '(absent)' });
  });
  it('keeps only changes and their context, with a gap between distant ones', () => {
    const a = Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`k${i}`, i]));
    const h = diffHunks(jsonDiff(a, { ...a, k0: 'x', k11: 'y' }), 1);
    expect(h.filter((l) => l === null)).toHaveLength(1);
    expect(h.filter((l) => l && l.op !== ' ').length).toBe(4);
    expect(diffHunks(jsonDiff({ a: 1 }, { a: 1 }))).toEqual([]);
  });
});
