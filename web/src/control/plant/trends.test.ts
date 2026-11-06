import { describe, expect, it } from 'vitest';
import { agentLinks, flowsOf } from './flows';
import { actionRange, filterActions, lanesOf, placeLane } from './lanes';
import type { PlantAction, SystemUnit } from './model';
import { contextSeries, duration, latency, ratesBySystem, totals } from './trends';

const act = (id: string, at: number, over: Partial<PlantAction> = {}): PlantAction =>
  ({ id, at, agent: 'a', system: 'erp', tool: 't', effect: 'read', effectBasis: 'declared', outcome: 'ok', ...over });
const ERP: SystemUnit = { id: 'erp', kind: 'erp', name: 'ERPNext', basis: 'declared' };
const MIN = 60_000;

describe('trends', () => {
  it('counts actions per minute per system, with reads, writes and abnormal ones', () => {
    const now = 15 * MIN;
    const [r] = ratesBySystem([ERP], [act('1', now - 30_000), act('2', now - 20_000, { effect: 'write', outcome: 'refused' }), act('3', now - 2 * MIN - 1)], now);
    expect(r!.perMin.slice(-3)).toEqual([1, 0, 2]);
    expect([r!.reads, r!.writes, r!.abnormal]).toEqual([2, 1, 1]);
  });

  it('context per call, totals that say when not every report was priced, and latency quantiles', () => {
    expect(contextSeries([{ at: 1, context: 10 }, { at: 2, tokens: 5 }, { at: 3, context: 30 }])).toEqual({ values: [10, 30], peak: 30 });
    expect(totals([{ at: 1, tokens: 10, costUsd: 0.5 }, { at: 2, tokens: 5 }])).toEqual({ tokens: 15, costUsd: 0.5, priced: false });
    expect(totals([])).toEqual({ priced: true });
    expect(latency([act('1', 0, { ms: 100 }), act('2', 0, { ms: 300 }), act('3', 0)])).toEqual({ count: 2, median: 300, p90: 300, recent: [100, 300] });
    expect([duration(420), duration(3200), duration(125_000)]).toEqual(['420 ms', '3.2 s', '2 min 5 s']);
  });
});

describe('flows and lanes', () => {
  it('a flow per agent and system, live while recent; messages to a teammate link agents', () => {
    const now = 10 * MIN;
    const f = flowsOf([act('1', now - 5 * MIN), act('2', now - 10_000, { effect: 'write' }), act('3', now - 5_000, { effect: 'unknown', outcome: 'error' })], now);
    expect(f).toEqual([{ agent: 'a', system: 'erp', reads: 1, writes: 1, other: 1, abnormal: 1, last: now - 5_000, live: true }]);
    const links = agentLinks([act('m', now - 2 * MIN, { system: 'msg', to: 'b' }), act('n', now, { system: 'msg', to: '*' })], ['a', 'b'], now);
    expect(links).toEqual([{ from: 'a', to: 'b', count: 1, last: now - 2 * MIN, live: false }]);
  });

  it('places marks so close ones go to the next row, and keeps lane order stable', () => {
    const r = { start: 0, end: 100_000 };
    expect(placeLane([act('1', 1000), act('2', 2000), act('3', 50_000)], r).map((m) => [m.action.id, m.row])).toEqual([['1', 0], ['2', 1], ['3', 0]]);
    const lanes = lanesOf([act('1', 1000, { agent: 'z' }), act('2', 2000)], ['a', 'b'], r, new Set(['b']));
    expect(lanes.map((l) => [l.agent, l.marks.length])).toEqual([['a', 1], ['z', 1]]);
    expect(actionRange([], 5)).toBeNull();
    expect(actionRange([act('1', 1000, { ms: 500 })], 0)).toEqual({ start: 1000, end: 1500 });
  });

  it('filters by system, writes only and abnormal only', () => {
    const list = [act('1', 0), act('2', 0, { effect: 'write' }), act('3', 0, { system: 'db', outcome: 'error' })];
    expect(filterActions(list, { writesOnly: true }).map((a) => a.id)).toEqual(['2']);
    expect(filterActions(list, { abnormalOnly: true }).map((a) => a.id)).toEqual(['3']);
    expect(filterActions(list, { system: 'erp' }).map((a) => a.id)).toEqual(['1', '2']);
  });
});
