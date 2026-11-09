/**
 * Declarative dragging (Horowitz, Blinn and Heer, "Declarative Dragging", 2026; the Dragology
 * library), sized for Sculptured's board.
 *
 * A drag does not move pixels. It lists the model states it can produce, each with where the
 * dragged element lands in it (a Target), and a Behavior picks, for every pointer position, the
 * state to preview, and to keep on release. Invalid states are never listed, so they can never be
 * previewed or dropped. Dragology draws SVG from pure render functions; the board's cards are
 * React components positioned straight from the model, so here a state's "render" is the model's
 * own geometry and interpolation (`board.ts`, `lerpDraft`) blends that.
 */

export type Point = { x: number; y: number };
/** A reachable state, where the dragged element lands in it, and a stable name for it. */
export type Target<T> = { state: T; at: Point; key: string };
/** One frame of a drag: the target to preview (and drop), and where the dragged element is drawn. */
export type Frame<T> = Target<T> & { drawAt: Point };
/** `p`: where the dragged element would be if it followed the pointer exactly. */
export type Behavior<T> = (p: Point) => Frame<T> | null;

export const dist = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const at = <T>(t: Target<T> | null): Frame<T> | null => t && { ...t, drawAt: t.at };

/**
 * `closest`: the target whose landing is nearest the pointer. The current one stays until another
 * is `stick` closer, so a pointer resting on a boundary does not flicker between two previews.
 */
export function closest<T>(targets: readonly Target<T>[], stick = 0): Behavior<T> {
  let current: Target<T> | null = null;
  return (p) => {
    let best: Target<T> | null = null;
    let bestDist = Infinity;
    for (const t of targets) {
      const d = dist(p, t.at);
      if (d < bestDist) [best, bestDist] = [t, d];
    }
    if (current && best && current !== best && dist(p, current.at) - bestDist < stick) best = current;
    current = best;
    return at(best);
  };
}

/**
 * `vary` for a canvas: a continuous family of states, one per position. `target` answers null
 * where that position is not a valid state.
 */
export const free = <T>(target: (p: Point) => Target<T> | null): Behavior<T> => (p) => at(target(p));

/**
 * `near.whenFar(far)`: `near` while its landing is within `radius` of the pointer. Beyond that,
 * whichever of the two lands nearer the pointer (`far` usually lands right under it).
 */
export const whenFar = <T>(near: Behavior<T>, far: Behavior<T>, radius: number): Behavior<T> => (p) => {
  const n = near(p);
  if (n && dist(p, n.at) <= radius) return n;
  const f = far(p);
  return !f || (n && dist(p, n.at) < dist(p, f.at)) ? n : f;
};

/** `withFloating`: the dragged element follows the pointer while everything else previews the landing. */
export const withFloating = <T>(b: Behavior<T>): Behavior<T> => (p) => {
  const f = b(p);
  return f && { ...f, drawAt: p };
};

/** `withSnapRadius`: within `radius` of its landing the drawn element is pulled toward it, fully at the landing. */
export const withSnapRadius = <T>(b: Behavior<T>, radius: number): Behavior<T> => (p) => {
  const f = b(p);
  if (!f) return f;
  const d = dist(f.drawAt, f.at);
  if (d >= radius) return f;
  const t = (1 - d / radius) ** 2;
  return { ...f, drawAt: { x: lerp(f.drawAt.x, f.at.x, t), y: lerp(f.drawAt.y, f.at.y, t) } };
};

/** Keeps the last valid frame where the behavior has none: an invalid spot is never offered. */
export function sticky<T>(b: Behavior<T>): Behavior<T> {
  let last: Frame<T> | null = null;
  return (p) => (last = b(p) ?? last);
}

/** Targets with the same key are one state: the first is kept. */
export function distinct<T>(targets: Iterable<Target<T> | null>): Target<T>[] {
  const seen = new Map<string, Target<T>>();
  for (const t of targets) if (t && !seen.has(t.key)) seen.set(t.key, t);
  return [...seen.values()];
}
