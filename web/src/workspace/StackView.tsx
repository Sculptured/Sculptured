import { createContext, memo, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react';
import { DropOverlay } from './DropOverlay';
import type { Node, PaneRef, Stack } from './model';
import { PaneMenu, type MoveTarget } from './PaneMenu';
import type { PaneType, Registry } from './registry';
import type { DragState, DropTarget } from './useDrag';

export type Actions = {
  activate(stackId: string, index: number): void;
  close(paneId: string): void;
  duplicate(paneId: string): void;
  move(paneId: string, target: DropTarget): void;
  setSettings(paneId: string, settings: Record<string, unknown>): void;
  resize(splitId: string, index: number, delta: number): void;
  sizeOf(splitId: string, index: number): number;
  popOut(paneId: string): void;
};

export type WorkspaceApi = {
  registry: Registry;
  act: Actions;
  drag: DragState | null;
  startDrag(e: React.PointerEvent, paneId: string): void;
  /** Puts the stack's pane bodies into its body slot, showing the active one. */
  place(slot: HTMLElement, list: PaneRef[], active: number): void;
  targets(stackId: string): MoveTarget[];
  canPopOut: boolean;
};

export const WorkspaceContext = createContext<WorkspaceApi | null>(null);
const useWorkspace = () => useContext(WorkspaceContext)!;

export const titleOf = (registry: Registry, pane: PaneRef | undefined) => (pane ? (registry.get(pane.type)?.title ?? 'Unknown pane') : 'Empty');

/** The title a node goes by: its first stack's active pane. */
export function nodeTitle(registry: Registry, node: Node): string {
  return node.kind === 'stack' ? titleOf(registry, node.panes[node.active]) : nodeTitle(registry, node.kids[0]);
}

function usePaneSettings(pane: PaneRef, type: PaneType | undefined, act: Actions) {
  const settings = useMemo(() => ({ ...type?.defaultSettings, ...pane.settings }), [type, pane.settings]);
  const setSettings = useCallback((s: Record<string, unknown>) => act.setSettings(pane.id, s), [act, pane.id]);
  return { settings, setSettings };
}

/** A pane's body. Rendered once per pane id, so live updates and moves never remount it. */
export const PaneBody = memo(function PaneBody({ pane, type, act }: { pane: PaneRef; type: PaneType | undefined; act: Actions }) {
  const { settings, setSettings } = usePaneSettings(pane, type, act);
  if (!type) return <p className="ws-empty">This kind of pane is no longer available. Close it with ✕.</p>;
  return <type.Body instance={pane.id} settings={settings} setSettings={setSettings} />;
});

function SettingsPanel({ pane, type, done }: { pane: PaneRef; type: PaneType; done(): void }) {
  const { act } = useWorkspace();
  const { settings, setSettings } = usePaneSettings(pane, type, act);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => box.current?.querySelector<HTMLElement>('input, select, textarea, button')?.focus(), []);
  const Form = type.Settings!;
  return (
    <div ref={box} className="ws-settings" role="group" aria-label={`${type.title} settings`}>
      <Form instance={pane.id} settings={settings} setSettings={setSettings} />
      <button type="button" className="btn btn-sm" onClick={done}>Done</button>
    </div>
  );
}

function Tab({ stack, index }: { stack: Stack; index: number }) {
  const { registry, act, drag, startDrag } = useWorkspace();
  const pane = stack.panes[index];
  const on = index === stack.active;
  const title = titleOf(registry, pane);
  return (
    <div className={`ws-tab${on ? ' is-on' : ''}${drag?.paneId === pane.id ? ' is-dragged' : ''}`}>
      <button
        type="button"
        role="tab"
        id={`ws-tab-${pane.id}`}
        data-tab={pane.id}
        aria-selected={on}
        aria-controls={`ws-panel-${pane.id}`}
        className="ws-tab-name"
        onPointerDown={(e) => startDrag(e, pane.id)}
        onClick={() => { if (!on) act.activate(stack.id, index); }}
      >
        <span className="ws-glyph" aria-hidden="true">{registry.get(pane.type)?.glyph ?? '∅'}</span>
        <span>{title}</span>
      </button>
      <button type="button" className="ws-tab-close" aria-label={`Close ${title}`} onClick={() => act.close(pane.id)}>✕</button>
    </div>
  );
}

type MenuProps = { stack: Stack; pane: PaneRef; opener: RefObject<HTMLButtonElement | null>; close(refocus: boolean): void; edit(id: string): void };

function StackMenu({ stack, pane, opener, close, edit }: MenuProps) {
  const { registry, act, targets, canPopOut } = useWorkspace();
  const type = registry.get(pane.type);
  const run = (fn: () => void) => () => { close(false); fn(); };
  return (
    <PaneMenu
      paneTitle={titleOf(registry, pane)}
      targets={targets(stack.id)}
      canDuplicate={!!type?.multi}
      hasSettings={!!type?.Settings}
      canPopOut={canPopOut}
      opener={opener}
      onMove={(t) => { close(false); act.move(pane.id, t); }}
      onDuplicate={run(() => act.duplicate(pane.id))}
      onSettings={run(() => edit(pane.id))}
      onPopOut={run(() => act.popOut(pane.id))}
      onClosePane={run(() => act.close(pane.id))}
      onDismiss={close}
    />
  );
}

function useBodySlot(stack: Stack) {
  const { place } = useWorkspace();
  const slot = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (slot.current) place(slot.current, stack.panes, stack.active);
  }, [place, stack.panes, stack.active]);
  return slot;
}

/** One stack of tabbed panes: the tab strip with its `⋯` menu, inline settings, then the active body. */
export function StackView({ stack }: { stack: Stack }) {
  const { registry, drag } = useWorkspace();
  const [menu, setMenu] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const slot = useBodySlot(stack);
  const pane = stack.panes[stack.active] as PaneRef | undefined;
  const type = pane && registry.get(pane.type);
  const over = drag?.over?.stackId === stack.id ? drag.over.zone : null;
  const close = (refocus: boolean) => { setMenu(false); if (refocus) opener.current?.focus(); };
  return (
    <section className="ws-stack" data-stack={stack.id} aria-label={titleOf(registry, pane)}>
      <div className="ws-bar">
        <div role="tablist" className="ws-tablist" aria-label="Panes here">
          {stack.panes.map((p, i) => <Tab key={p.id} stack={stack} index={i} />)}
        </div>
        {pane && (
          <button ref={opener} type="button" className="ws-more" aria-label={`${titleOf(registry, pane)} options`} aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((m) => !m)}>⋯</button>
        )}
      </div>
      {menu && pane && <StackMenu stack={stack} pane={pane} opener={opener} close={close} edit={setEditing} />}
      {pane && type?.Settings && editing === pane.id && <SettingsPanel pane={pane} type={type} done={() => setEditing(null)} />}
      {pane ? <div ref={slot} className="ws-body" /> : <p className="ws-empty">Empty. Use Add pane.</p>}
      {over && <DropOverlay zone={over} />}
    </section>
  );
}
