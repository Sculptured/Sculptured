import { describe, expect, it } from 'vitest';
import { branchB1, ev } from '../simHarness';
import { arenaPlant, TEAM_MESSAGES } from './arenaPlant';

describe('a Shakedown run as a plant', () => {
  it('uses the recorded systems and tool effects', () => {
    const p = arenaPlant(branchB1());
    expect(p.systems.find((s) => s.id === 'retail')).toMatchObject({ kind: 'erp', name: 'ERPNext', standsInFor: 'SAP S/4HANA', simulated: false, basis: 'declared' });
    expect(p.actions[0]).toMatchObject({ system: 'retail', effect: 'read', effectBasis: 'declared' });
  });

  it('does not infer systems or effects from old world declarations, names or writes', () => {
    const b = branchB1();
    b.events[0]!.payload = { world: [{ part: 'desk', integration: 'sculptured' }] };
    const actions = arenaPlant(b).actions.filter((a) => a.tool !== 'handoff');
    expect(actions.every((a) => a.system === 'unknown' && a.effect === 'unknown' && a.ms === undefined)).toBe(true);
  });

  it('prefers the call part, then the tool part, only when a system was declared', () => {
    const b = branchB1();
    b.events.push(...['plant', 'undeclared', undefined].map((part, i) =>
      ev(20 + i, 20 + i, 'resolver', 'tool_call', { name: 'get_order_details', part })));
    expect(arenaPlant(b).actions.slice(-3).map((a) => a.system)).toEqual(['plant', 'retail', 'retail']);
  });

  it('keeps recorded durations including zero; absent durations stay unknown', () => {
    const b = branchB1();
    b.events.push(...[0, 420, undefined].map((duration_ms, i) =>
      ev(20 + i, 20 + i, 'resolver', 'tool_call', { name: 'get_order_details', duration_ms })));
    expect(arenaPlant(b).actions.slice(-3).map((a) => a.ms)).toEqual([0, 420, undefined]);
  });

  it('a handoff is a write to the team messages; a tool error is a refusal or an error by its own words', () => {
    const b = branchB1();
    b.events.push(
      ev(12, 20, 'resolver', 'tool_call', { name: 'exchange_delivered_order_items', args: {}, result: null, error: 'refused: the agent is read-only', part: 'retail', reads: [], writes: [] }),
      ev(13, 21, 'resolver', 'tool_call', { name: 'get_order_details', args: {}, result: null, error: 'connection reset', part: 'retail', reads: [], writes: [] }),
    );
    const p = arenaPlant(b);
    expect(p.actions.find((a) => a.tool === 'handoff')).toMatchObject({ system: TEAM_MESSAGES, effect: 'write', to: 'next' });
    expect(p.actions.slice(-2).map((a) => [a.outcome, a.reason])).toEqual([['refused', 'refused: the agent is read-only'], ['error', 'connection reset']]);
  });

  it('permits come from the approvals part: waiting, then granted with who decided; the asking action carries it', () => {
    const p = arenaPlant(branchB1());
    expect(p.permits.map((x) => [x.id, x.state, x.by ?? null, x.agent])).toEqual([['c1', 'waiting', null, 'triage'], ['c2', 'granted', 'person', 'resolver']]);
    expect(p.actions.find((a) => a.id === 's3')?.permit).toBe('c1');
  });

  it("reads each model call's context, tokens and cost, and each agent's read-only right", () => {
    const b = branchB1();
    b.team.agents[0] = { ...b.team.agents[0]!, ...{ read_only: true } };
    b.events.push(ev(14, 22, 'triage', 'model_call', { usage: { prompt_tokens: 1200, completion_tokens: 80 }, cost_usd: 0.01 }));
    const p = arenaPlant(b);
    expect(p.usage).toEqual([{ at: Date.parse('2026-11-02T10:00:22Z'), agent: 'triage', context: 1200, tokens: 1280, costUsd: 0.01 }]);
    expect(p.rights.triage).toEqual(['read only: Shakedown gives it only read tools']);
    expect(p.rights.resolver).toEqual([]);
  });
});
