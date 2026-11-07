import { useEffect, useRef, type ReactNode, type RefObject } from 'react';

type MenuProps = {
  label: string;
  opener: RefObject<HTMLElement | null>;
  className?: string;
  /** `refocus` is true when the menu was left by keyboard, so focus goes back to its button. */
  onDismiss(refocus: boolean): void;
  children: ReactNode;
};

/** A menu's enabled items, in order. */
export const enabledItems = (box: HTMLElement | null) => [...(box?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [])];

/** Calls `dismiss` on a pointer press outside the menu and its opener. */
export function useDismissOutside(box: RefObject<HTMLElement | null>, opener: RefObject<HTMLElement | null>, dismiss: () => void) {
  const latest = useRef(dismiss);
  useEffect(() => { latest.current = dismiss; });
  useEffect(() => {
    const away = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!box.current?.contains(t) && !opener.current?.contains(t)) latest.current();
    };
    window.addEventListener('pointerdown', away, true);
    return () => window.removeEventListener('pointerdown', away, true);
  }, [box, opener]);
}

/** A menu's way through: `sub` (and `open`, `back`) for a menu with a submenu ("Move to…"). */
export type MenuNav = { dismiss(refocus: boolean): void; sub?: boolean; open?(): void; back?(): void };

/**
 * Every menu's keys: ↑/↓/Home/End move, → opens a submenu, ← or Esc goes back from one, Esc on
 * the top level closes (focus back to its button), Tab leaves.
 */
export function menuKeys(box: RefObject<HTMLElement | null>, nav: MenuNav) {
  return (e: React.KeyboardEvent) => {
    const list = enabledItems(box.current);
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const go = (n: number) => { e.preventDefault(); list[(n + list.length) % list.length]?.focus(); };
    const moves: Record<string, () => void> = { ArrowDown: () => go(at + 1), ArrowUp: () => go(at - 1), Home: () => go(0), End: () => go(list.length - 1) };
    if (moves[e.key] && list.length) return moves[e.key]();
    levelKey(e, nav);
  };
}

/** The keys that change menu level: → opens a submenu, ← or Esc goes back or closes, Tab leaves. */
function levelKey(e: React.KeyboardEvent, nav: MenuNav) {
  const inSub = nav.sub && nav.back;
  if (e.key === 'ArrowRight' && nav.open && !nav.sub && document.activeElement?.hasAttribute('aria-haspopup')) { e.preventDefault(); nav.open(); }
  else if (e.key === 'ArrowLeft' && inSub) { e.preventDefault(); nav.back!(); }
  else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (inSub) nav.back!(); else nav.dismiss(true); }
  else if (e.key === 'Tab') nav.dismiss(false);
}

/** A small popup menu of `role="menuitem"` buttons. Focuses its first enabled item when it opens. */
export function Menu({ label, opener, className, onDismiss, children }: MenuProps) {
  const box = useRef<HTMLDivElement>(null);
  useDismissOutside(box, opener, () => onDismiss(false));
  useEffect(() => { enabledItems(box.current)[0]?.focus(); }, []);
  return (
    <div ref={box} role="menu" aria-label={label} className={`ws-menu ${className ?? ''}`} onKeyDown={menuKeys(box, { dismiss: onDismiss })}>
      {children}
    </div>
  );
}
