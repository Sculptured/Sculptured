import { useEffect, useRef, useState } from 'react';
import { Menu } from './Menu';
import type { PageLayouts } from './model';

export type LayoutActions = {
  activate(id: string): void;
  add(): void;
  rename(id: string, name: string): void;
  duplicate(id: string): void;
  remove(id: string): void;
  reset(id: string): void;
};

/** Inline rename: Enter or leaving the field keeps the name, Escape cancels. */
function RenameField({ name, done }: { name: string; done(name: string | null): void }) {
  const finished = useRef(false);
  const finish = (value: string | null) => { if (!finished.current) { finished.current = true; done(value); } };
  return (
    <input
      className="workspace-rename"
      aria-label="Layout name"
      defaultValue={name}
      autoFocus
      onFocus={(e) => e.currentTarget.select()}
      onBlur={(e) => finish(e.currentTarget.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') { e.preventDefault(); finish(e.currentTarget.value); }
        else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(null); }
      }}
    />
  );
}

type MenuProps = { value: PageLayouts; act: LayoutActions; opener: React.RefObject<HTMLButtonElement | null>; close(refocus: boolean): void; edit(): void };

function LayoutMenu({ value, act, opener, close, edit }: MenuProps) {
  const id = value.active;
  const only = value.layouts.length < 2;
  const run = (fn: () => void) => () => { close(false); fn(); };
  return (
    <Menu label="Layout options" opener={opener} className="workspace-layout-menu" onDismiss={close}>
      <button type="button" role="menuitem" className="ws-menu-item" onClick={run(edit)}>Rename</button>
      <button type="button" role="menuitem" className="ws-menu-item" onClick={run(() => act.duplicate(id))}>Duplicate</button>
      <button type="button" role="menuitem" className="ws-menu-item" onClick={run(() => act.reset(id))}>Reset to default</button>
      <div className="ws-menu-sep" role="separator" />
      <button type="button" role="menuitem" className="ws-menu-item" disabled={only} onClick={run(() => act.remove(id))}>
        Delete{only && <span className="ws-menu-why"> (the only layout)</span>}
      </button>
    </Menu>
  );
}

/** Focuses a layout's tab once a rename ends, since the field it replaced had focus. */
function useRefocus(box: React.RefObject<HTMLElement | null>) {
  const [want, setWant] = useState<string | null>(null);
  useEffect(() => {
    if (!want) return;
    box.current?.querySelector<HTMLElement>(`[data-layout="${CSS.escape(want)}"]`)?.focus();
    setWant(null);
  }, [box, want]);
  return setWant;
}

/** Layouts as tabs: switch, "+" for a new one, double-click or the ⋯ menu to rename, duplicate, reset or delete. */
export function LayoutTabs({ value, act }: { value: PageLayouts; act: LayoutActions }) {
  const [editing, setEditing] = useState<string | null>(null);
  const [menu, setMenu] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const refocus = useRefocus(box);
  const done = (id: string) => (name: string | null) => { if (name !== null) act.rename(id, name); setEditing(null); refocus(id); };
  const close = (back: boolean) => { setMenu(false); if (back) opener.current?.focus(); };
  return (
    <div ref={box} className="workspace-layouts">
      <div role="tablist" aria-label="Layouts" className="workspace-tablist">
        {value.layouts.map((l) => (editing === l.id ? <RenameField key={l.id} name={l.name} done={done(l.id)} /> : (
          <button key={l.id} type="button" role="tab" data-layout={l.id} aria-selected={l.id === value.active} className="workspace-tab"
            onClick={() => act.activate(l.id)} onDoubleClick={() => setEditing(l.id)}>{l.name}</button>
        )))}
      </div>
      <button type="button" className="workspace-icon" aria-label="Add layout" title="Add layout" onClick={act.add}>+</button>
      <button ref={opener} type="button" className="workspace-icon" aria-label="Layout options" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((m) => !m)}>⋯</button>
      {menu && <LayoutMenu value={value} act={act} opener={opener} close={close} edit={() => setEditing(value.active)} />}
    </div>
  );
}
