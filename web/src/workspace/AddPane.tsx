import { useRef, useState } from 'react';
import { Menu } from './Menu';
import { panes, type Layout } from './model';
import type { PaneType, Registry } from './registry';
import { isSlot } from './slots';

/** The pane types on screen in this layout, in the tree or popped out. */
export function shownTypes(layout: Layout): Set<string> {
  return new Set([...panes(layout.root).filter((p) => !isSlot(p)), ...layout.popped].map((p) => p.type));
}

type Props = { registry: Registry; ctx: unknown; layout: Layout; onChoose(type: PaneType): void };

/**
 * "Add pane": every type that applies here. One already on screen that allows a single copy says
 * "shown", and choosing it focuses it instead of adding another.
 */
export function AddPane({ registry, ctx, layout, onChoose }: Props) {
  const [open, setOpen] = useState(false);
  const opener = useRef<HTMLButtonElement>(null);
  const shown = shownTypes(layout);
  const close = (refocus: boolean) => { setOpen(false); if (refocus) opener.current?.focus(); };
  return (
    <div className="workspace-add">
      <button ref={opener} type="button" className="btn btn-sm" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        + Add pane
      </button>
      {open && (
        <Menu label="Add pane" opener={opener} className="workspace-add-menu" onDismiss={close}>
          {registry.list(ctx).map((t) => {
            const on = shown.has(t.type) && !t.multi;
            return (
              <button key={t.type} type="button" role="menuitem" className="ws-menu-item" onClick={() => { close(false); onChoose(t); }}>
                <span className="ws-glyph" aria-hidden="true">{t.glyph}</span> {t.title}
                {on && <span className="ws-menu-why"> · shown</span>}
              </button>
            );
          })}
        </Menu>
      )}
    </div>
  );
}
