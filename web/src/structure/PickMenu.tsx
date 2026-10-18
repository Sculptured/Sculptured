import { useEffect, useRef } from 'react';

export type PickOption = { id: string; label: string; note?: string };

/**
 * A small menu of choices. Focus starts on the first item; ↑/↓ move, Enter picks, Esc closes
 * and returns focus to the button that opened it.
 */
export function PickMenu({
  label,
  options,
  onPick,
  onClose,
  empty,
}: {
  label: string;
  options: PickOption[];
  onPick: (id: string) => void;
  onClose: () => void;
  empty: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);
  useEffect(() => {
    opener.current = document.activeElement;
    ref.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    // Esc closes the menu wherever focus is (an empty menu leaves it on its button), before anything else hears it.
    const esc = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      onClose();
    };
    window.addEventListener('pointerdown', away);
    window.addEventListener('keydown', esc, true);
    return () => {
      window.removeEventListener('pointerdown', away);
      window.removeEventListener('keydown', esc, true);
      if (opener.current instanceof HTMLElement && document.contains(opener.current) && !ref.current?.contains(document.activeElement))
        opener.current.focus();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
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
