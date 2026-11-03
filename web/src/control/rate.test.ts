import { describe, expect, it } from 'vitest';
import { canCompare, choice, MUST_FINISH } from '../arena/compareRule';
import { rawNote } from './panes/RawEvents';
import { perMinute, RATE_MINUTES } from './rate';

const MIN = 60_000;

describe('tail honesty', () => {
  it('counts calls per minute, oldest first, and minutes before the loaded events are unknown', () => {
    const end = 100 * MIN;
    const all = perMinute([end - 10, end - 10, end - 2 * MIN - 1], end);
    expect(all).toHaveLength(RATE_MINUTES);
    expect(all.slice(-3)).toEqual([1, 0, 2]);
    const cut = perMinute([end - 10], end, end - 5 * MIN);
    expect(cut.filter((n) => n === null)).toHaveLength(RATE_MINUTES - 5);
    expect(cut.slice(-5)).toEqual([0, 0, 0, 0, 1]);
  });

  it('raw events say when they are only the latest ones', () => {
    expect(rawNote(10, false)).toBe('The event journal: what was recorded, verbatim, newest first.');
    expect(rawNote(10, true)).toMatch(/^Latest 10 recorded events/);
    expect(rawNote(5000, false)).toMatch(/^Latest 300 recorded events/);
  });

  it('both Compare panes refuse a running branch the same way', () => {
    expect(canCompare('passed', 'failed')).toBe(true);
    expect(canCompare('passed', 'running')).toBe(false);
    expect(choice('b2', 'running')).toEqual({ label: 'b2 (running)', disabled: true });
    expect(MUST_FINISH).toMatch(/must finish/);
  });
});
