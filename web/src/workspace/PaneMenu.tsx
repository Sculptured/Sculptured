import { useEffect, useRef, useState, type RefObject } from 'react';
import { enabledItems, menuKeys, useDismissOutside } from './Menu';
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
  useEffect(() => { enabledItems(box.current)[sub ? 1 : 0]?.focus(); }, [sub]);
  const nav = { sub, open: () => setSub(true), back: () => setSub(false), dismiss: props.onDismiss };
  return (
    <div ref={box} role="menu" aria-label={`${props.paneTitle} options`} className="ws-menu" onKeyDown={menuKeys(box, nav)}>
      {sub ? <MoveItems targets={props.targets} onMove={props.onMove} back={nav.back} /> : <MainItems {...props} open={nav.open} />}
    </div>
  );
}
