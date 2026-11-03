import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { trackPointer } from '../dom';
import type { Zone } from './model';

/** Fraction of a stack's width or height, from each edge, that drops beside it rather than into it. */
export const EDGE = 0.25;

export type DropTarget = { stackId: string; zone: Zone };
export type DragState = { paneId: string; over: DropTarget | null };
type Callbacks = {
  /** True when dropping there would change nothing, so no target is shown. */
  noop(paneId: string, target: DropTarget): boolean;
  drop(paneId: string, target: DropTarget): void;
};

/** The zone of a box that a point falls in: the nearest edge within EDGE of it, else the centre. */
export function zoneAt(box: DOMRect, x: number, y: number): Zone {
  const fx = (x - box.left) / box.width;
  const fy = (y - box.top) / box.height;
  const near: [Zone, number][] = [['left', fx], ['right', 1 - fx], ['top', fy], ['bottom', 1 - fy]];
  const [zone, d] = near.reduce((a, b) => (b[1] < a[1] ? b : a));
  return d < EDGE ? zone : 'center';
}

/** The stack element under a point, found by box so it works without hit-testing support. */
function targetAt(root: HTMLElement | null, x: number, y: number): DropTarget | null {
  for (const el of root?.querySelectorAll<HTMLElement>('[data-stack]') ?? []) {
    const box = el.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0) continue;
    if (x < box.left || x > box.right || y < box.top || y > box.bottom) continue;
    return { stackId: el.dataset.stack!, zone: zoneAt(box, x, y) };
  }
  return null;
}

/** Escape, heard before anything else on the page, while a drag is on. Returns its removal. */
function onEscape(cancel: () => void): () => void {
  const onKey = (ev: KeyboardEvent) => {
    if (ev.key !== 'Escape') return;
    ev.preventDefault();
    ev.stopPropagation();
    cancel();
  };
  window.addEventListener('keydown', onKey, true);
  return () => window.removeEventListener('keydown', onKey, true);
}

/**
 * Dragging a pane by its tab. Past the 4 px threshold, the stack under the pointer shows where
 * the pane would land; release drops it there and Escape cancels. The callbacks are read when
 * used, so a re-render mid-drag (a live update) never interrupts the drag.
 */
export function useDrag(root: RefObject<HTMLElement | null>, callbacks: Callbacks) {
  const [drag, setDrag] = useState<DragState | null>(null);
  const cbs = useRef(callbacks);
  const stop = useRef<(() => void) | null>(null);
  useEffect(() => { cbs.current = callbacks; });
  useEffect(() => () => stop.current?.(), []);

  const start = useCallback((e: React.PointerEvent, paneId: string) => {
    if (e.button !== 0) return;
    stop.current?.();
    let over: DropTarget | null = null;
    let shown = false;
    const onMove = (_dx: number, _dy: number, ev: PointerEvent) => {
      const hit = targetAt(root.current, ev.clientX, ev.clientY);
      const next = hit && !cbs.current.noop(paneId, hit) ? hit : null;
      // Re-render only when the target changes, not on every pointer move.
      if (shown && next?.stackId === over?.stackId && next?.zone === over?.zone) return;
      over = next;
      shown = true;
      setDrag({ paneId, over });
    };
    const onEnd = (moved: boolean) => {
      finish();
      if (moved && over) cbs.current.drop(paneId, over);
    };
    const untrack = trackPointer(e, onMove, onEnd);
    const unkey = onEscape(() => finish());
    const finish = () => { untrack(); unkey(); stop.current = null; setDrag(null); };
    stop.current = finish;
  }, [root]);

  return { drag, start };
}
