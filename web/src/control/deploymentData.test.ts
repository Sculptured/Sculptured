import { describe, expect, it } from 'vitest';
import { oneAtATime } from './deploymentData';

describe('oneAtATime', () => {
  it('never loads twice at once, and loads once more after calls made meanwhile', async () => {
    const pending: ((n: number) => void)[] = [];
    let calls = 0;
    const load = oneAtATime(() => { calls += 1; return new Promise<number>((r) => pending.push(r)); });
    const first = load();
    const second = load();
    const third = load();
    expect(calls).toBe(1);
    expect(second).toBe(third);
    pending[0](1);
    await expect(first).resolves.toBe(1);
    await new Promise((r) => setTimeout(r, 0));
    expect(calls).toBe(2);
    pending[1](2);
    await expect(second).resolves.toBe(2);
    // Idle again: the next call loads at once.
    void load();
    expect(calls).toBe(3);
  });
});
