import { describe, expect, it } from 'vitest';
import { closest, distinct, free, sticky, whenFar, withFloating, withSnapRadius, type Target } from './spec';

const t = (key: string, x: number, y: number): Target<string> => ({ state: key, at: { x, y }, key });
const A = t('A', 0, 0);
const B = t('B', 100, 0);

describe('declarative drag operators', () => {
  it('closest previews the state whose landing is nearest the pointer, and draws the element there', () => {
    const b = closest([A, B]);
    expect(b({ x: 30, y: 0 })).toMatchObject({ key: 'A', drawAt: { x: 0, y: 0 } });
    expect(b({ x: 70, y: 5 })).toMatchObject({ key: 'B', drawAt: { x: 100, y: 0 } });
    expect(closest([])({ x: 0, y: 0 })).toBeNull();
  });

  it('closest sticks to the current state until another is clearly closer', () => {
    const b = closest([A, B], 10);
    expect(b({ x: 10, y: 0 })?.key).toBe('A');
    expect(b({ x: 54, y: 0 })?.key).toBe('A');
    expect(b({ x: 56, y: 0 })?.key).toBe('B');
  });

  it('whenFar keeps the snapped state near its landing, else the free one, else whichever lands nearer', () => {
    const anywhere = free((p) => t(`free@${p.x}`, p.x, p.y));
    const b = whenFar(closest([A]), anywhere, 20);
    expect(b({ x: 15, y: 0 })?.key).toBe('A');
    expect(b({ x: 60, y: 0 })?.key).toBe('free@60');
    // A free placement that is not valid here is never offered: the snapped state stays.
    const nowhere = whenFar(closest([A]), free(() => null), 20);
    expect(nowhere({ x: 60, y: 0 })?.key).toBe('A');
    // A free placement nudged further away than the snap target loses to it.
    const pushed = whenFar(closest([A]), free((p) => t('pushed', p.x + 200, p.y)), 20);
    expect(pushed({ x: 60, y: 0 })?.key).toBe('A');
  });

  it('withFloating draws the element under the pointer while previewing the landing', () => {
    const f = withFloating(closest([A, B]))({ x: 30, y: 12 });
    expect(f).toMatchObject({ key: 'A', at: { x: 0, y: 0 }, drawAt: { x: 30, y: 12 } });
  });

  it('withSnapRadius pulls the drawn element toward its landing, all the way at the landing', () => {
    const b = withSnapRadius(withFloating(closest([A])), 40);
    expect(b({ x: 0, y: 0 })?.drawAt).toEqual({ x: 0, y: 0 });
    const half = b({ x: 20, y: 0 })!.drawAt.x;
    expect(half).toBeGreaterThan(0);
    expect(half).toBeLessThan(20);
    expect(b({ x: 50, y: 0 })?.drawAt).toEqual({ x: 50, y: 0 });
  });

  it('sticky keeps the last valid frame where nothing is valid', () => {
    const b = sticky(free((p) => (p.x < 50 ? t(`at${p.x}`, p.x, 0) : null)));
    expect(b({ x: 80, y: 0 })).toBeNull();
    expect(b({ x: 40, y: 0 })?.key).toBe('at40');
    expect(b({ x: 80, y: 0 })?.key).toBe('at40');
  });

  it('distinct keeps one target per key and drops invalid ones', () => {
    expect(distinct([A, null, t('A', 9, 9), B]).map((x) => x.at)).toEqual([{ x: 0, y: 0 }, { x: 100, y: 0 }]);
  });
});
