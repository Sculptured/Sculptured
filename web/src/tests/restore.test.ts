import { describe, expect, it } from 'vitest';
import {
  APPROXIMATE_LABEL, isApproximate, notComparedLine, planLine, savedLine, savedParts, worldLine, worldParts,
} from './restore';
import type { Event } from './types';

describe('planLine', () => {
  it('says how each part is restored, in plain words', () => {
    expect(planLine({ part: 'retail', how: 'save' })).toBe('retail: from its save');
    expect(planLine({ part: 'plant', how: 'replay', estimate_seconds: 40.3 })).toBe('plant: replayed from the start, about 40 s');
    expect(planLine({ part: 'plant', how: 'replay', estimate_seconds: 0.2 })).toBe('plant: replayed from the start, about 1 s');
    expect(planLine({ part: 'plant', how: 'replay', estimate_seconds: 300 })).toBe('plant: replayed from the start, about 5 min');
    expect(planLine({ part: 'plant', how: 'replay' })).toBe('plant: replayed from the start');
    expect(planLine({ part: 'isaac', how: 'approximate' })).toBe('isaac: approximate');
  });
});

describe('isApproximate', () => {
  it('marks a branch only when a part was restored approximately', () => {
    expect(APPROXIMATE_LABEL).toBe('Approximate restore');
    expect(isApproximate({ restored: [{ part: 'retail', how: 'save' }, { part: 'isaac', how: 'approximate' }] })).toBe(true);
    // Asked for approximate, but every part had an exact save: the rerun is exact.
    expect(isApproximate({ restore_mode: 'approximate', restored: [{ part: 'retail', how: 'save' }] })).toBe(false);
    expect(isApproximate({ restored: [] })).toBe(false);
    expect(isApproximate({})).toBe(false);
  });
});

describe('notComparedLine', () => {
  it('names the parts left out, so they never read as no differences', () => {
    expect(notComparedLine(['plant', 'isaac'], 'compare')).toBe('Not compared: plant, isaac (they keep no saves)');
    expect(notComparedLine(['plant'], 'compare')).toBe('Not compared: plant (it keeps no saves)');
    expect(notComparedLine(['plant'], 'check')).toBe('Not compared: plant (no expected result to compare with)');
    expect(notComparedLine([], 'compare')).toBe('');
    expect(notComparedLine(undefined, 'check')).toBe('');
  });
});

describe('saved after a step', () => {
  it('reads saves, falling back to a legacy snapshot', () => {
    expect(savedParts({ saves: { plant: 's1', retail: 's2' } })).toEqual(['plant', 'retail']);
    expect(savedParts({ snapshot: 'snap_a' })).toEqual(['retail']);
    expect(savedParts({ snapshot: null })).toEqual([]);
    expect(savedLine({ saves: { retail: 's2' }, snapshot: 's2' })).toBe('Saved after this step');
    expect(savedLine({ saves: { plant: 's1', retail: 's2' } })).toBe('Saved after this step: plant, retail');
    expect(savedLine({ snapshot: 'snap_a' })).toBe('Saved after this step');
    expect(savedLine({})).toBe('');
  });
});

describe('worldParts', () => {
  const start = (payload: unknown): Event[] => [{ branch_id: 'b', seq: 1, at: '', agent: null, kind: 'run_start', payload }];
  it('lists the world parts with what their fingerprints pin', () => {
    const evs = start({
      task_id: 't1',
      world: [{ part: 'retail', integration: 'postgres' }, { part: 'plant', integration: 'counter' }],
      fingerprints: { retail: { server_version: '16.4' }, plant: { example: 'counter', gpu: { name: 'x' } } },
    });
    expect(worldParts(evs)).toEqual([
      { part: 'retail', integration: 'postgres', version: 'server_version 16.4' },
      { part: 'plant', integration: 'counter', version: 'example counter' },
    ]);
    expect(worldLine(evs)).toBe('World: retail (postgres · server_version 16.4) · plant (counter · example counter)');
  });
  it('is empty for a run recorded before integrations', () => {
    expect(worldParts(start({ task_id: '27' }))).toEqual([]);
    expect(worldLine(start({ task_id: '27' }))).toBe('');
    expect(worldParts([])).toEqual([]);
  });
});
