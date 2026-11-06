import { useEffect, useImperativeHandle, useMemo, useRef, useState, type Dispatch, type ReactNode, type Ref, type RefObject, type SetStateAction } from 'react';
import { AddPane } from './AddPane';
import { addLayout, deleteLayout, duplicateLayout, poppedOf, renameLayout, resetLayout, withNothingPopped, withPopped } from './layouts';
import { LayoutTabs, type LayoutActions } from './LayoutTabs';
import { MobileWorkspace, useMobile } from './MobileWorkspace';
import { addPane, freshId, lookAttr, panes, setActive, stacks, type Layout, type Node, type PageLayouts, type WorkspaceLook } from './model';
import type { PaneType, Registry } from './registry';
import { SelectionProvider, useChannelPost } from './selection';
import { shellRegistry, SlotContext } from './slots';
import { loadLayouts, loadPopped, saveLayouts, savePopped } from './store';
import { closeWindowsOf, usePopOut, useSetRoot } from './usePopOut';
import { Workspace } from './Workspace';

export type WorkspacePageProps = {
  /** Names the saved layouts and the selection channel: one per page ("control", "arena"). */
  page: string;
  /**
   * The selection channel when it is not the page's: many workspaces sharing one page's layouts (each
   * Arena run) talk only among their own windows. The layouts are shared; which panes are popped out
   * is each channel's own (another tab on another channel never takes them back or closes them).
   */
  channel?: string;
  /** Made once per page (module level): its pane types are wrapped once and kept. */
  registry: Registry;
  ctx: unknown;
  defaults: () => PageLayouts;
  /** Above the layout tabs: the page's own header (alarms, stop controls). */
  header?: ReactNode;
  /** The route a popped-out pane opens at, such as `/p/team/control/pane/<instance>`. */
  paneRoute(instance: string): string;
  /** For the page's own controls (the header's master caution): bring a pane type forward. */
  handle?: Ref<WorkspaceHandle>;
};

/** `show(type)`: focuses a pane of that type (any copy), shows its window if popped out, or adds one. */
export type WorkspaceHandle = { show(type: string): void };

type SetValue = Dispatch<SetStateAction<PageLayouts>>;
type Post = ReturnType<typeof useChannelPost>;

/** The page's layouts. Shared by many channels, they are saved with nothing popped out, and each channel's popped panes apart. */
function usePageLayouts(page: string, defaults: () => PageLayouts, registry: Registry, channel?: string) {
  const shared = channel !== undefined && channel !== page ? channel : null;
  const [value, setValue] = useState(() => {
    const loaded = loadLayouts(page, defaults, (t) => !!registry.get(t));
    return shared ? withPopped(withNothingPopped(loaded), loadPopped(page, shared)) : loaded;
  });
  useEffect(() => {
    if (!shared) return saveLayouts(page, value);
    saveLayouts(page, withNothingPopped(value));
    savePopped(page, shared, poppedOf(value));
  }, [page, shared, value]);
  const layout = value.layouts.find((l) => l.id === value.active) ?? value.layouts[0];
  return { value, setValue, layout };
}

/** Reset and Delete drop a layout's popped panes, so their windows are told to close. */
function layoutActions(value: PageLayouts, setValue: SetValue, defaults: () => PageLayouts, post: Post): LayoutActions {
  const discard = (id: string) => closeWindowsOf(post, value.layouts.find((l) => l.id === id));
  return {
    activate: (id) => setValue((v) => (v.layouts.some((l) => l.id === id) ? { ...v, active: id } : v)),
    add: () => setValue((v) => addLayout(v, defaults())),
    rename: (id, name) => setValue((v) => renameLayout(v, id, name)),
    duplicate: (id) => setValue((v) => duplicateLayout(v, id)),
    remove: (id) => { if (value.layouts.length > 1) discard(id); setValue((v) => deleteLayout(v, id)); },
    reset: (id) => { discard(id); setValue((v) => resetLayout(v, id, defaults())); },
  };
}

/** Focuses a pane's tab once it is on screen (after Add pane). */
function useFocusTab(box: RefObject<HTMLElement | null>) {
  const [want, setWant] = useState<string | null>(null);
  useEffect(() => {
    if (!want) return;
    box.current?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(want)}"]`)?.focus();
    setWant(null);
  }, [box, want]);
  return setWant;
}

/** Add pane: a single-copy type already on screen is focused (or its window shown); otherwise added to the focused stack. */
function chooser(layout: Layout, setRoot: (root: Node) => void, focused: RefObject<string | undefined>, show: (id: string) => void, reopen: (id: string) => void) {
  return (t: PaneType) => {
    const there = !t.multi && panes(layout.root).find((p) => p.type === t.type);
    const stack = there && stacks(layout.root).find((s) => s.panes.includes(there));
    if (there && stack) { setRoot(setActive(layout.root, stack.id, stack.panes.indexOf(there))); show(there.id); return; }
    const away = !t.multi && layout.popped.find((p) => p.type === t.type);
    if (away) { reopen(away.id); return; }
    const id = freshId();
    setRoot(addPane(layout.root, { id, type: t.type }, focused.current));
    show(id);
  };
}

function LookSetting({ value, setValue }: { value: PageLayouts; setValue: SetValue }) {
  return (
    <label className="workspace-look">
      Look
      <select value={value.look ?? 'plant'} onChange={(e) => setValue((v) => ({ ...v, look: e.target.value as WorkspaceLook }))}>
        <option value="plant">Plant</option>
        <option value="mission">Mission</option>
        <option value="app">Follow app look</option>
      </select>
    </label>
  );
}

function Notice({ text, dismiss }: { text: string | null; dismiss(): void }) {
  if (!text) return null;
  return (
    <p className="workspace-notice" role="status">
      {text} <button type="button" className="btn btn-sm btn-ghost" onClick={dismiss}>Dismiss</button>
    </p>
  );
}

type MainProps = { layout: Layout; registry: Registry; ctx: unknown; current: string | null; onCurrent(id: string): void; setRoot(root: Node): void; popOut(id: string): void; focused: RefObject<string | undefined> };

/** The docking area, or one pane at a time on mobile. Remembers the stack that last had focus, for Add pane. */
function Main({ layout, registry, ctx, current, onCurrent, setRoot, popOut, focused }: MainProps) {
  const mobile = useMobile();
  const onFocus = (e: React.FocusEvent) => { focused.current = (e.target as HTMLElement).closest<HTMLElement>('[data-stack]')?.dataset.stack ?? focused.current; };
  return (
    <div className="workspace-main" onFocus={onFocus}>
      {mobile
        ? <MobileWorkspace root={layout.root} registry={registry} ctx={ctx} current={current} onCurrent={onCurrent} onChange={setRoot} />
        : <Workspace key={layout.id} root={layout.root} registry={registry} ctx={ctx} onChange={setRoot} onPopOut={popOut} />}
    </div>
  );
}

/** One page's workspace: header, layout tabs, Add pane and Look, then the docking area. */
function PageShell(props: WorkspacePageProps) {
  const registry = useMemo(() => shellRegistry(props.registry), [props.registry]);
  const { value, setValue, layout } = usePageLayouts(props.page, props.defaults, registry, props.channel);
  const box = useRef<HTMLDivElement>(null);
  const focused = useRef<string | undefined>(undefined);
  const setRoot = useSetRoot(layout, setValue, focused);
  const post = useChannelPost();
  const [current, setCurrent] = useState<string | null>(null);
  const focusTab = useFocusTab(box);
  const pop = usePopOut(props.paneRoute, layout, setValue, focused);
  const show = (id: string) => { setCurrent(id); focusTab(id); };
  const choose = chooser(layout, setRoot, focused, show, pop.slots.reopen);
  useImperativeHandle(props.handle, () => ({ show: (type) => { const t = registry.get(type); if (t) choose({ ...t, multi: false }); } }));
  return (
    <SlotContext.Provider value={pop.slots}>
      <div ref={box} className="workspace" data-look={lookAttr(value.look)}>
        {props.header && <div className="workspace-header">{props.header}</div>}
        <div className="workspace-bar">
          <LayoutTabs value={value} act={layoutActions(value, setValue, props.defaults, post)} />
          <AddPane registry={registry} ctx={props.ctx} layout={layout} onChoose={choose} />
          <LookSetting value={value} setValue={setValue} />
        </div>
        <Notice text={pop.notice} dismiss={pop.dismiss} />
        <Main layout={layout} registry={registry} ctx={props.ctx} current={current} onCurrent={setCurrent} setRoot={setRoot} popOut={pop.popOut} focused={focused} />
      </div>
    </SlotContext.Provider>
  );
}

/**
 * A page's dockable workspace. Its selection channel also decides which window shows a popped-out
 * pane. Remounts when `page` or `channel` changes, so each keeps its own layouts and channel.
 */
export function WorkspacePage(props: WorkspacePageProps) {
  const channel = props.channel ?? props.page;
  return <SelectionProvider key={`${props.page}|${channel}`} page={channel}><PageShell {...props} /></SelectionProvider>;
}
