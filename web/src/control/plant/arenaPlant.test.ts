import { describe, expect, it } from 'vitest';
import type { BranchDetail, WorldPartInfo } from '../../tests/types';
import { branchB1, ev } from '../simHarness';
import { arenaPlant, TEAM_MESSAGES } from './arenaPlant';

/** b1 with each part's describe.system and its tools' effects kept in the run's world (as Arena keeps them). */
function declared(): BranchDetail {
  const b = branchB1();
  const start = b.events[0]!;
  start.payload = {
    task_id: '27',
    world: [
      { part: 'retail', integration: 'mongo', system: { kind: 'erp', product: 'ERPNext', stands_in_for: 'SAP S/4HANA', simulated: false },
        tools: [{ name: 'get_order_details', effect: 'read' }, { name: 'exchange_delivered_order_items', effect: 'write' }] },
      { part: 'plant', integration: 'counter', system: { kind: 'plant', product: 'Isaac Sim', simulated: true } },
      { part: 'desk', integration: 'sculptured' },
    ],
  };
  return b;
}

describe('an Arena run as a plant', () => {
  it('names each system from what the part declares, and says "unknown" when it does not', () => {
    const p = arenaPlant(branchB1(), null);
    expect(p.systems.map((s) => [s.id, s.kind, s.basis])).toEqual([
      ['retail', 'unknown', 'unknown'], ['plant', 'unknown', 'unknown'], ['desk', 'approvals', 'sculptured'], [TEAM_MESSAGES, 'messaging', 'declared'],
    ]);
    const d = arenaPlant(declared(), null).systems.find((s) => s.id === 'retail')!;
    expect(d).toMatchObject({ kind: 'erp', name: 'ERPNext', standsInFor: 'SAP S/4HANA', simulated: false, basis: 'declared' });
  });

  it('takes the effect a tool declares; without one, rows written are an observed write and nothing else is guessed', () => {
    const effects = (b: BranchDetail) => arenaPlant(b, null).actions.filter((a) => a.system === 'retail').map((a) => [a.tool, a.effect, a.effectBasis]);
    expect(effects(branchB1())).toEqual([['get_order_details', 'unknown', 'unknown'], ['exchange_delivered_order_items', 'write', 'observed']]);
    expect(effects(declared())).toEqual([['get_order_details', 'read', 'declared'], ['exchange_delivered_order_items', 'write', 'declared']]);
  });

  it("uses today's world only for a part that runs the same integration", () => {
    const now: WorldPartInfo[] = [
      { name: 'retail', integration: 'mongo', answered: true, ...{ system: { kind: 'database', product: 'MongoDB', simulated: false } } },
      { name: 'plant', integration: 'isaac', answered: true, ...{ system: { kind: 'plant', product: 'Isaac Sim', simulated: true } } },
    ];
    const kinds = arenaPlant(branchB1(), now).systems.map((s) => [s.id, s.kind]);
    expect(kinds).toContainEqual(['retail', 'database']);
    expect(kinds).toContainEqual(['plant', 'unknown']);
  });

  it('a handoff is a write to the team messages; a tool error is a refusal or an error by its own words', () => {
    const b = branchB1();
    b.events.push(
      ev(12, 20, 'resolver', 'tool_call', { name: 'exchange_delivered_order_items', args: {}, result: null, error: 'refused: the agent is read-only', part: 'retail', reads: [], writes: [] }),
      ev(13, 21, 'resolver', 'tool_call', { name: 'get_order_details', args: {}, result: null, error: 'connection reset', part: 'retail', reads: [], writes: [] }),
    );
    const p = arenaPlant(b, null);
    expect(p.actions.find((a) => a.tool === 'handoff')).toMatchObject({ system: TEAM_MESSAGES, effect: 'write', to: 'next' });
    expect(p.actions.slice(-2).map((a) => [a.outcome, a.reason])).toEqual([['refused', 'refused: the agent is read-only'], ['error', 'connection reset']]);
  });

  it('permits come from the approvals part: waiting, then granted with who decided; the asking action carries it', () => {
    const p = arenaPlant(branchB1(), null);
    expect(p.permits.map((x) => [x.id, x.state, x.by ?? null, x.agent])).toEqual([['c1', 'waiting', null, 'triage'], ['c2', 'granted', 'person', 'resolver']]);
    expect(p.actions.find((a) => a.id === 's3')?.permit).toBe('c1');
  });

  it("reads each model call's context, tokens and cost, and each agent's read-only right", () => {
    const b = branchB1();
    b.team.agents[0] = { ...b.team.agents[0]!, ...{ read_only: true } };
    b.events.push(ev(14, 22, 'triage', 'model_call', { usage: { prompt_tokens: 1200, completion_tokens: 80 }, cost_usd: 0.01 }));
    const p = arenaPlant(b, null);
    expect(p.usage).toEqual([{ at: Date.parse('2026-11-02T10:00:22Z'), agent: 'triage', context: 1200, tokens: 1280, costUsd: 0.01 }]);
    expect(p.rights.triage).toEqual(['read only: Arena gives it only read tools']);
    expect(p.rights.resolver).toEqual([]);
  });
});
