import { useEffect, useState } from 'react';
import type { Box } from '../layout';

/**
 * How agents are drawn on the board, Design and Run alike: as cards with their details, or as
 * their provider's logo. Per browser, never sent to the server. Positions never depend on it.
 */
export type BoardStyle = 'cards' | 'logos';
export const BOARD_STYLES: Record<BoardStyle, string> = { cards: 'Cards', logos: 'Logos' };
export const BOARD_STYLE_KEY = 'sculptured.board.style.v1';
const CHANGED = 'sculptured:board-style';

export function readBoardStyle(): BoardStyle {
  try {
    return globalThis.localStorage?.getItem(BOARD_STYLE_KEY) === 'logos' ? 'logos' : 'cards';
  } catch {
    return 'cards';
  }
}

/** Switch every board in this tab (and, through storage, in other tabs). */
export function setBoardStyle(style: BoardStyle) {
  try {
    globalThis.localStorage?.setItem(BOARD_STYLE_KEY, style);
  } catch {
    // A per-browser convenience only: this tab still switches.
  }
  window.dispatchEvent(new CustomEvent<BoardStyle>(CHANGED, { detail: style }));
}

/** The board style on screen, kept current when it changes here (the switch, Ctrl K) or in another tab. */
export function useBoardStyle(): BoardStyle {
  const [style, setStyle] = useState<BoardStyle>(readBoardStyle);
  useEffect(() => {
    const changed = (e: Event) => setStyle((e as CustomEvent<BoardStyle>).detail ?? readBoardStyle());
    const sync = (e: StorageEvent) => { if (e.key === BOARD_STYLE_KEY) setStyle(readBoardStyle()); };
    window.addEventListener(CHANGED, changed);
    window.addEventListener('storage', sync);
    return () => { window.removeEventListener(CHANGED, changed); window.removeEventListener('storage', sync); };
  }, []);
  return style;
}

/**
 * Where a logo node's arrows end, inside its unchanged 200 × 200 slot: a column around the logo,
 * its name and its status, centred on the slot so its sides meet arrows at the slot's middle.
 * The slot (and so every position) is the same in both styles; only the arrows' ends move in.
 */
export const LOGO_ROUTE_W = 96;
export const LOGO_ROUTE_TOP = 24;
export function logoRouteBox(b: Box): Box {
  return { x: b.x + (b.w - LOGO_ROUTE_W) / 2, y: b.y + LOGO_ROUTE_TOP, w: LOGO_ROUTE_W, h: b.h - 2 * LOGO_ROUTE_TOP };
}

/** Agents' boxes for routing arrows in Logos style. Groups (`\0group:`) and workflow boxes (`$…`) keep theirs. */
export function logoRouteBoxes(boxes: Record<string, Box>): Record<string, Box> {
  return Object.fromEntries(Object.entries(boxes).map(([id, b]) => [id, id.startsWith('\0') || id.startsWith('$') ? b : logoRouteBox(b)]));
}

function mediaMatches(query: string): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia(query).matches;
}

/** Whether the user asked for less motion, kept current. */
export function useReducedMotion(): boolean {
  const query = '(prefers-reduced-motion: reduce)';
  const [reduced, setReduced] = useState(() => mediaMatches(query));
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const m = window.matchMedia(query);
    const on = () => setReduced(m.matches);
    m.addEventListener?.('change', on);
    return () => m.removeEventListener?.('change', on);
  }, []);
  return reduced;
}

/** False while the tab is hidden, so looping logos stop costing anything. */
export function usePageVisible(): boolean {
  const [visible, setVisible] = useState(() => typeof document === 'undefined' || document.visibilityState !== 'hidden');
  useEffect(() => {
    const on = () => setVisible(document.visibilityState !== 'hidden');
    document.addEventListener('visibilitychange', on);
    return () => document.removeEventListener('visibilitychange', on);
  }, []);
  return visible;
}

/** Working logos may move: motion is not reduced and the tab is visible. Off-screen nodes are not drawn at all (culling). */
export function useLogoMotion(): boolean {
  const reduced = useReducedMotion();
  const visible = usePageVisible();
  return !reduced && visible;
}
