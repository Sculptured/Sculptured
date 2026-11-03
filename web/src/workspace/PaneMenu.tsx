import { useEffect, useRef, useState, type RefObject } from 'react';
import type { Zone } from './model';
import type { DropTarget } from './useDrag';

/** A stack the pane can move beside: another stack by its active pane's title, or its own (edges only). */
export type MoveTarget = { stackId: string; title: string; own?: boolean };

export type PaneMenuProps = {
  paneTitle: string;
  targets: MoveTarget[];
  canDuplicate: boolean;
  hasSettings: boolean;
  canPopOut: boolean;
  opener: RefObject<HTMLElement | null>;
  onMove(target: DropTarget): void;
  onDuplicate(): void;
  onSettings(): void;
  onPopOut(): void;
  onClosePane(): void;
  /** `refocus` is true when the menu was left by keyboard, so focus goes back to its button. */
  onDismiss(refocus: boolean): void;
};

const ZONES: { zone: Zone; short: string; name: (t: string) => string }[] = [
  { zone: 'left', short: 'Left', name: (t) => `Left of ${t}` },
  { zone: 'right', short: 'Right', name: (t) => `Right of ${t}` },
  { zone: 'top', short: 'Above', name: (t) => `Above ${t}` },
  { zone: 'bottom', short: 'Below', name: (t) => `Below ${t}` },
  { zone: 'center', short: 'As a tab', name: (t) => `As a tab with ${t}` },
];

const enabled = (box: HTMLElement | null) => [...(box?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [])];

function useDismissOutside(box: RefObject<HTMLElement | null>, opener: RefObject<HTMLElement | null>, dismiss: () => void) {
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

type Nav = { sub: boolean; open(): void; back(): void; dismiss(refocus: boolean): void };

/** ↑/↓/Home/End move, → opens "Move to…", ← or Esc goes back, Esc on the top level closes, Tab leaves. */
function keyHandler(box: RefObject<HTMLElement | null>, nav: Nav) {
  return (e: React.KeyboardEvent) => {
    const items = enabled(box.current);
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const go = (n: number) => { e.preventDefault(); items[(n + items.length) % items.length]?.focus(); };
    const moves: Record<string, () => void> = { ArrowDown: () => go(at + 1), ArrowUp: () => go(at - 1), Home: () => go(0), End: () => go(items.length - 1) };
    if (moves[e.key] && items.length) return moves[e.key]();
    if (e.key === 'ArrowRight' && !nav.sub && document.activeElement?.hasAttribute('aria-haspopup')) { e.preventDefault(); nav.open(); }
    else if (e.key === 'ArrowLeft' && nav.sub) { e.preventDefault(); nav.back(); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (nav.sub) nav.back(); else nav.dismiss(true); }
    else if (e.key === 'Tab') nav.dismiss(false);
  };
}

function Item({ name, onClick, children, ...rest }: { name?: string; onClick(): void; children: React.ReactNode; disabled?: boolean; 'aria-haspopup'?: 'menu' }) {
  return (
    <button type="button" role="menuitem" className="ws-menu-item" aria-label={name} onClick={onClick} {...rest}>
      {children}
    </button>
  );
}

function MoveItems({ targets, onMove, back }: { targets: MoveTarget[]; onMove(t: DropTarget): void; back(): void }) {
  return (
    <>
      <Item onClick={back}>‹ Back</Item>
      {targets.map((t) => (
        <div key={t.stackId} role="group" aria-label={t.own ? 'This group' : t.title} className="ws-menu-group">
          <div className="ws-menu-head" aria-hidden="true">{t.own ? 'This group' : t.title}</div>
          {ZONES.filter((z) => !t.own || z.zone !== 'center').map((z) => (
            <Item key={z.zone} name={z.name(t.own ? 'this group' : t.title)} onClick={() => onMove({ stackId: t.stackId, zone: z.zone })}>
              {z.short}
            </Item>
          ))}
        </div>
      ))}
    </>
  );
}

function MainItems(p: PaneMenuProps & { open(): void }) {
  const none = p.targets.length === 0;
  return (
    <>
      <Item aria-haspopup="menu" disabled={none} onClick={p.open}>
        Move to…{none && <span className="ws-menu-why"> (nothing to move beside)</span>}
      </Item>
      {p.canDuplicate && <Item onClick={p.onDuplicate}>Duplicate</Item>}
      {p.hasSettings && <Item onClick={p.onSettings}>Settings</Item>}
      {p.canPopOut && <Item onClick={p.onPopOut}>Pop out</Item>}
      <div className="ws-menu-sep" role="separator" />
      <Item onClick={p.onClosePane}>Close</Item>
    </>
  );
}

/** The `⋯` menu of a pane: move it (the keyboard path for every drag), duplicate, settings, pop out, close. */
export function PaneMenu(props: PaneMenuProps) {
  const box = useRef<HTMLDivElement>(null);
  const [sub, setSub] = useState(false);
  useDismissOutside(box, props.opener, () => props.onDismiss(false));
  useEffect(() => { enabled(box.current)[sub ? 1 : 0]?.focus(); }, [sub]);
  const nav: Nav = { sub, open: () => setSub(true), back: () => setSub(false), dismiss: props.onDismiss };
  return (
    <div ref={box} role="menu" aria-label={`${props.paneTitle} options`} className="ws-menu" onKeyDown={keyHandler(box, nav)}>
      {sub ? <MoveItems targets={props.targets} onMove={props.onMove} back={nav.back} /> : <MainItems {...props} open={nav.open} />}
    </div>
  );
}
