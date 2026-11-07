import { useEffect, useRef, type RefObject } from 'react';

export type PickOption = { id: string; label: string; note?: string };

/** Focus moves into the menu while it is open and returns to what opened it; a press outside or Esc closes it. */
function useMenuFocus(ref: RefObject<HTMLDivElement | null>, onClose: () => void) {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const menu = ref.current;
    const opener = document.activeElement;
    menu?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const away = (e: PointerEvent) => { if (!menu?.contains(e.target as Node)) close.current(); };
    // Esc closes the menu wherever focus is (an empty menu leaves it on its button), before anything else hears it.
    const esc = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      close.current();
    };
    window.addEventListener('pointerdown', away);
    window.addEventListener('keydown', esc, true);
    return () => {
      window.removeEventListener('pointerdown', away);
      window.removeEventListener('keydown', esc, true);
      if (opener instanceof HTMLElement && document.contains(opener) && !menu?.contains(document.activeElement)) opener.focus();
    };
  }, [ref]);
}

/** ↑/↓ move between the items (wrapping), Esc closes, Tab leaves. */
function menuKeys(root: RefObject<HTMLDivElement | null>, onClose: () => void) {
  return (e: React.KeyboardEvent) => {
    const items = [...(root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (items.length === 0) return;
      const n = e.key === 'ArrowDown' ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
      items[n].focus();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === 'Tab') {
      onClose();
    }
  };
}

/**
 * A small menu of choices. Focus starts on the first item; ↑/↓ move, Enter picks, Esc closes
 * and returns focus to the button that opened it.
 */
export function PickMenu({ label, options, onPick, onClose, empty }: { label: string; options: PickOption[]; onPick: (id: string) => void; onClose: () => void; empty: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useMenuFocus(ref, onClose);
  const onKeyDown = menuKeys(ref, onClose);

  return (
    <div className="pick-menu" role="menu" aria-label={label} ref={ref} onKeyDown={onKeyDown}>
      {options.length === 0 ? (
        <p className="pick-empty muted" role="none">
          {empty}
        </p>
      ) : (
        options.map((o) => (
          <button
            key={o.id}
            type="button"
            role="menuitem"
            className="pick-item"
            onClick={() => {
              onPick(o.id);
              onClose();
            }}
          >
            <span className="pick-label">{o.label}</span>
            {o.note && <span className="pick-note muted">{o.note}</span>}
          </button>
        ))
      )}
    </div>
  );
}
