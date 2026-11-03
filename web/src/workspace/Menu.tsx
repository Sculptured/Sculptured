import { useEffect, useRef, type ReactNode, type RefObject } from 'react';
import { useDismissOutside } from './PaneMenu';

type MenuProps = {
  label: string;
  opener: RefObject<HTMLElement | null>;
  className?: string;
  /** `refocus` is true when the menu was left by keyboard, so focus goes back to its button. */
  onDismiss(refocus: boolean): void;
  children: ReactNode;
};

const items = (box: HTMLElement | null) => [...(box?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [])];

/** ↑/↓/Home/End move between items, Esc closes and returns focus, Tab leaves. */
function onKey(box: RefObject<HTMLElement | null>, dismiss: (refocus: boolean) => void) {
  return (e: React.KeyboardEvent) => {
    const list = items(box.current);
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const go = (n: number) => { e.preventDefault(); list[(n + list.length) % list.length]?.focus(); };
    const moves: Record<string, () => void> = { ArrowDown: () => go(at + 1), ArrowUp: () => go(at - 1), Home: () => go(0), End: () => go(list.length - 1) };
    if (moves[e.key] && list.length) moves[e.key]();
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); dismiss(true); }
    else if (e.key === 'Tab') dismiss(false);
  };
}

/** A small popup menu of `role="menuitem"` buttons. Focuses its first enabled item when it opens. */
export function Menu({ label, opener, className, onDismiss, children }: MenuProps) {
  const box = useRef<HTMLDivElement>(null);
  useDismissOutside(box, opener, () => onDismiss(false));
  useEffect(() => { items(box.current)[0]?.focus(); }, []);
  return (
    <div ref={box} role="menu" aria-label={label} className={`ws-menu ${className ?? ''}`} onKeyDown={onKey(box, onDismiss)}>
      {children}
    </div>
  );
}
