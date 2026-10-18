import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { trackPointer } from '../dom';
import type { Box } from '../layout';

export type Viewport = { x: number; y: number; k: number };
/** Everything drawn, in world units. */
export type Bounds = { x0: number; y0: number; x1: number; y1: number };

/** Viewports per team survive leaving a board (agent pane, other teams, the other board). */
const viewports = new Map<string, Viewport>();
/** Teams whose viewport the user panned or zoomed: auto-fit leaves those alone. */
const touched = new Set<string>();

/** Boards asked to keep their view through the next resize (a panel docking beside them), and when. */
const held = new Map<string, number>();
const HOLD_MS = 1000;

export function forgetViewports() {
  viewports.clear();
  touched.clear();
  held.clear();
}

/**
 * The board's area is about to change size because of a panel beside it (Run finished docking or
 * folding), not because the window did: keep the view as it is, so no card moves. If the drawing
 * would no longer fit and the user has not panned or zoomed, it fits again as usual.
 */
export function holdViewport(key: string) {
  held.set(key, Date.now());
}

/** Every part of the drawing is inside a `w` × `h` area at this view. */
function shows(v: Viewport, b: Bounds, w: number, h: number): boolean {
  return b.x0 * v.k + v.x >= 0 && b.y0 * v.k + v.y >= 0 && b.x1 * v.k + v.x <= w && b.y1 * v.k + v.y <= h;
}

export const MIN_K = 0.02;
export const MAX_K = 2;
/** Auto-fit scales up to fill the area, but not past this: card text stays near the UI's own
 * reading size (about 15px at the default 13px) instead of ballooning on a large screen. */
export const MAX_FIT_K = 1.15;
const MARGIN = 48;
/** Breathing room around a fitted drawing. */
const FIT_MARGIN = 28;
const clampK = (k: number) => Math.min(MAX_K, Math.max(MIN_K, k));
export const DEFAULT_VIEWPORT: Viewport = { x: MARGIN, y: MARGIN, k: 1 };

/** The viewport that shows all of `b` centred in a `w` × `h` area. */
export function fitViewport(b: Bounds, w: number, h: number, maxK = MAX_FIT_K): Viewport {
  const bw = Math.max(1, b.x1 - b.x0);
  const bh = Math.max(1, b.y1 - b.y0);
  const k = clampK(Math.min(w / (bw + FIT_MARGIN * 2), h / (bh + FIT_MARGIN * 2), maxK));
  return { k, x: (w - bw * k) / 2 - b.x0 * k, y: (h - bh * k) / 2 - b.y0 * k };
}

/**
 * Pan and zoom for a board, shared by the Structure board and the Live graph so a team keeps
 * one view across both. It fits on first view, when the team changes and when the area or the
 * drawing changes size, until the user pans or zooms; after that it never moves on its own.
 */
export function usePanZoom(key: string, bounds: Bounds | null, preserveOnGrowth = false) {
  const el = useRef<HTMLDivElement>(null);
  const [vp, setVpState] = useState<Viewport>(() => viewports.get(key) ?? DEFAULT_VIEWPORT);
  const vpRef = useRef(vp);
  const boundsRef = useRef(bounds);
  boundsRef.current = bounds;
  const panStop = useRef<(() => void) | null>(null);

  const setVp = useCallback(
    (next: Viewport | ((v: Viewport) => Viewport)) => {
      setVpState((cur) => {
        const v = typeof next === 'function' ? next(cur) : next;
        vpRef.current = v;
        viewports.set(key, v);
        return v;
      });
    },
    [key],
  );

  // Another team: its own remembered view.
  const lastKey = useRef(key);
  useLayoutEffect(() => {
    if (lastKey.current === key) return;
    lastKey.current = key;
    const v = viewports.get(key) ?? DEFAULT_VIEWPORT;
    vpRef.current = v;
    setVpState(v);
  }, [key]);

  const fitNow = useCallback(() => {
    const node = el.current;
    const b = boundsRef.current;
    if (!node || !b || !node.clientWidth || !node.clientHeight) return false;
    setVp(fitViewport(b, node.clientWidth, node.clientHeight));
    return true;
  }, [setVp]);

  /** `f` / Fit: fits now, and auto-fit applies again until the next pan or zoom. */
  const fit = useCallback(() => {
    touched.delete(key);
    fitNow();
  }, [key, fitNow]);

  const boundsKey = bounds ? `${bounds.x0},${bounds.y0},${bounds.x1},${bounds.y1}` : '';
  useLayoutEffect(() => {
    if (touched.has(key) || !bounds || (preserveOnGrowth && viewports.has(key))) return;
    fitNow();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, boundsKey, fitNow]);

  useEffect(() => {
    const node = el.current;
    if (!node || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      if (touched.has(key)) return;
      const since = held.get(key);
      held.delete(key);
      const b = boundsRef.current;
      if (since !== undefined && Date.now() - since < HOLD_MS && b && shows(vpRef.current, b, node.clientWidth, node.clientHeight)) return;
      fitNow();
    });
    ro.observe(node);
    return () => ro.disconnect();
  }, [key, fitNow]);

  const zoomAt = useCallback(
    (factor: number, cx?: number, cy?: number) => {
      touched.add(key);
      setVp((v) => {
        const node = el.current;
        const px = cx ?? (node?.clientWidth ?? 0) / 2;
        const py = cy ?? (node?.clientHeight ?? 0) / 2;
        const k = clampK(v.k * factor);
        const r = k / v.k;
        return { k, x: px - (px - v.x) * r, y: py - (py - v.y) * r };
      });
    },
    [key, setVp],
  );

  // Wheel zoom needs a non-passive listener to keep the page from scrolling.
  useEffect(() => {
    const node = el.current;
    if (!node) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = node.getBoundingClientRect();
      zoomAt(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top);
    };
    node.addEventListener('wheel', onWheel, { passive: false });
    return () => node.removeEventListener('wheel', onWheel);
  }, [zoomAt]);

  // A drag that is still going when the board unmounts stops listening.
  useEffect(() => () => panStop.current?.(), []);

  /** Starts panning from a pointer down on empty space. */
  const startPan = useCallback(
    (e: React.PointerEvent) => {
      const start = vpRef.current;
      el.current?.classList.add('panning');
      panStop.current?.();
      panStop.current = trackPointer(
        e,
        (dx, dy) => {
          touched.add(key);
          setVp({ ...start, x: start.x + dx, y: start.y + dy });
        },
        () => {
          panStop.current = null;
          el.current?.classList.remove('panning');
        },
      );
    },
    [key, setVp],
  );

  /** Keyboard focus on a box that is out of view pans just enough to show it. */
  const reveal = useCallback(
    (b: Box) => {
      const node = el.current;
      if (!node || !node.clientWidth) return;
      setVp((v) => {
        const left = v.x + b.x * v.k;
        const top = v.y + b.y * v.k;
        const right = left + b.w * v.k;
        const bottom = top + b.h * v.k;
        let { x, y } = v;
        if (left < 8) x += 8 - left;
        else if (right > node.clientWidth - 8) x -= right - (node.clientWidth - 8);
        if (top < 8) y += 8 - top;
        else if (bottom > node.clientHeight - 8) y -= bottom - (node.clientHeight - 8);
        if (x === v.x && y === v.y) return v;
        touched.add(key);
        return { ...v, x, y };
      });
    },
    [key, setVp],
  );

  /** A client point in world units. */
  const toWorld = useCallback((clientX: number, clientY: number) => {
    const r = el.current?.getBoundingClientRect();
    const v = vpRef.current;
    return { x: (clientX - (r?.left ?? 0) - v.x) / v.k, y: (clientY - (r?.top ?? 0) - v.y) / v.k };
  }, []);

  /** Editing a position or route is deliberate placement, so subsequent bounds must not refit. */
  const hold = useCallback(() => { touched.add(key); }, [key]);
  return { el, vp, vpRef, fit, zoomAt, startPan, reveal, toWorld, hold };
}
