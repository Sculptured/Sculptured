import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import type { Item } from '../conversation';
import { scrollWithin } from './highlight';

/** Within this many px of the end counts as "at the bottom". */
const STICK_PX = 24;

export function isTextField(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName);
}

export type Scroller = RefObject<HTMLDivElement | null>;
export type Stick = { atBottom: RefObject<boolean> };

/** Follow new output while at the bottom (unless a jump is pending); otherwise count it in a "▼ N new" chip. */
export function useStickToBottom(scroller: Scroller, items: Item[], focusKey: string | null, jumped: RefObject<string | null>) {
  const atBottom = useRef(true);
  const count = useRef(0);
  const [unread, setUnread] = useState(0);
  useLayoutEffect(() => {
    const el = scroller.current;
    const before = count.current;
    count.current = items.length;
    if (!el) return;
    const pendingJump = !!focusKey && jumped.current !== focusKey;
    if (atBottom.current && !pendingJump) el.scrollTop = el.scrollHeight;
    else if (items.length > before && before > 0) setUnread((n) => n + items.length - before);
  }, [items, focusKey, scroller, jumped]);
  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_PX;
    if (atBottom.current) setUnread(0);
  };
  const jump = () => {
    const el = scroller.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    atBottom.current = true;
    setUnread(0);
  };
  return { atBottom, unread, onScroll, jump };
}

/** Jump to a focused item once it has loaded: scroll it into view and flash it. */
export function useFocusJump(scroller: Scroller, focusKey: string | null, focus: unknown, shownCount: number, jumped: RefObject<string | null>, stick: Stick) {
  const [flashKey, setFlashKey] = useState<string | null>(null);
  useEffect(() => {
    if (!focusKey || jumped.current === focusKey || !focus) return;
    const el = scroller.current?.querySelector<HTMLElement>(`[data-item-key="${CSS.escape(focusKey)}"]`);
    if (!el) return;
    jumped.current = focusKey;
    stick.atBottom.current = false;
    scrollWithin(scroller.current, el);
    (el.closest('li') as HTMLElement | null)?.focus({ preventScroll: true });
    setFlashKey(focusKey);
    const t = setTimeout(() => setFlashKey(null), 2400);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusKey, focus, shownCount]);
  return flashKey;
}
