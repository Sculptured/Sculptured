import { useEffect, useReducer, useRef } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import { gap, lerpDraft } from './board';

/** Milliseconds for the drawn board to close about two thirds of the way to its target. */
const TAU = 55;

const motionAllowed = () =>
  typeof requestAnimationFrame === 'function' && !!globalThis.matchMedia?.('(prefers-reduced-motion: no-preference)').matches;

/**
 * The board as drawn while `on`: every change to `target` is animated (Dragology's transitions
 * between previews, and the drop). Cards and groups ease toward their new place each frame, so a
 * target that changes again mid-way is simply chased. `pin` (the floating card) is never eased.
 * Off, or with reduced motion, the target is drawn as it is.
 */
export function useTween(target: TeamDraft, on: boolean, pin: string | null): TeamDraft {
  const drawn = useRef(target);
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  const animate = on && motionAllowed();
  useEffect(() => {
    if (!animate) return;
    if (gap(drawn.current, target) < 0.5) {
      drawn.current = target;
      return;
    }
    let raf = 0;
    let last = performance.now();
    const step = (now: number) => {
      const next = lerpDraft(drawn.current, target, 1 - Math.exp(-Math.max(1, now - last) / TAU), pin);
      last = now;
      const done = gap(next, target) < 0.5;
      drawn.current = done ? target : next;
      redraw();
      if (!done) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [target, animate, pin]);
  if (!animate) drawn.current = target;
  return animate ? lerpDraft(drawn.current, target, 0, pin) : target;
}
