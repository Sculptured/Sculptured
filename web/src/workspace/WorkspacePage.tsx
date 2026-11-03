import { useEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type RefObject, type SetStateAction } from 'react';
import { AddPane } from './AddPane';
import { addLayout, bringBack, deleteLayout, duplicateLayout, eachLayout, popOutPane, renameLayout, resetLayout, setPoppedSettings } from './layouts';
import { LayoutTabs, type LayoutActions } from './LayoutTabs';
import { MobileWorkspace, useMobile } from './MobileWorkspace';
import { addPane, freshId, panes, setActive, stacks, type Layout, type Node, type PageLayouts } from './model';
import type { PaneType, Registry } from './registry';
import { SelectionProvider, type ChannelMessage } from './selection';
import { shellRegistry, slotFor, SlotContext, type SlotActions } from './slots';
import { loadLayouts, saveLayouts } from './store';
import { Workspace } from './Workspace';

export type WorkspacePageProps = {
  /** Names the saved layouts and the selection channel: one per page ("control", "arena"). */
  page: string;
  /** Made once per page (module level): its pane types are wrapped once and kept. */
  registry: Registry;
  ctx: unknown;
  defaults: () => PageLayouts;
  /** Above the layout tabs: the page's own header (alarms, stop controls). */
  header?: ReactNode;
  /** The route a popped-out pane opens at, such as `/p/team/control/pane/<instance>`. */
  paneRoute(instance: string): string;
};

type SetValue = Dispatch<SetStateAction<PageLayouts>>;

const BLOCKED = 'The browser blocked the new window. Allow pop-ups for this page, then choose Pop out again.';

function usePageLayouts(page: string, defaults: () => PageLayouts, registry: Registry) {
  const [value, setValue] = useState(() => loadLayouts(page, defaults, (t) => !!registry.get(t)));
  useEffect(() => saveLayouts(page, value), [page, value]);
  const layout = value.layouts.find((l) => l.id === value.active) ?? value.layouts[0];
  const setRoot = (root: Node) => setValue((v) => eachLayout(v, (l) => (l.id === layout.id ? { ...l, root } : l)));
  return { value, setValue, layout, setRoot };
}

function layoutActions(setValue: SetValue, defaults: () => PageLayouts): LayoutActions {
  return {
    activate: (id) => setValue((v) => (v.layouts.some((l) => l.id === id) ? { ...v, active: id } : v)),
    add: () => setValue((v) => addLayout(v, defaults())),
    rename: (id, name) => setValue((v) => renameLayout(v, id, name)),
    duplicate: (id) => setValue((v) => duplicateLayout(v, id)),
    remove: (id) => setValue((v) => deleteLayout(v, id)),
    reset: (id) => setValue((v) => resetLayout(v, id, defaults())),
  };
}

/** Pop out into a named window, bring back by button or when that window closes, and keep its settings. */
function usePopOut(paneRoute: (instance: string) => string, layout: Layout, setValue: SetValue, focused: RefObject<string | undefined>) {
  const [notice, setNotice] = useState<string | null>(null);
  const windows = useRef(new Map<string, Window>());
  const open = (instance: string) => {
    const known = windows.current.get(instance);
    if (known && !known.closed) { known.focus(); return true; }
    const win = window.open(paneRoute(instance), `sculptured-pane-${instance}`, 'popup,width=760,height=560');
    if (win) windows.current.set(instance, win);
    setNotice(win ? null : BLOCKED);
    return !!win;
  };
  const update = (fn: (l: Layout) => Layout) => setValue((v) => eachLayout(v, fn));
  const back = (id: string) => update((l) => bringBack(l, id, focused.current));
  // "Bring back" also closes the window, when this page opened it, so the pane is not shown twice.
  const slots: SlotActions = { bringBack: (id) => { windows.current.get(id)?.close?.(); windows.current.delete(id); back(id); }, reopen: (id) => void open(id) };
  const popOut = (paneId: string) => {
    const pane = panes(layout.root).find((p) => p.id === paneId);
    const slot = pane && slotFor(pane);
    if (!pane || slot !== undefined) return void (slot && open(slot));
    if (open(paneId)) update((l) => (l.id === layout.id ? popOutPane(l, paneId) : l));
  };
  const onMessage = (m: ChannelMessage) => {
    if (m.kind === 'back') back(m.instance);
    if (m.kind === 'settings') update((l) => setPoppedSettings(l, m.instance, m.settings));
  };
  return { popOut, slots, onMessage, notice, dismiss: () => setNotice(null) };
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
      <select value={value.look === 'app' ? 'app' : 'mission'} onChange={(e) => setValue((v) => ({ ...v, look: e.target.value === 'app' ? 'app' : 'mission' }))}>
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
  const { value, setValue, layout, setRoot } = usePageLayouts(props.page, props.defaults, registry);
  const box = useRef<HTMLDivElement>(null);
  const focused = useRef<string | undefined>(undefined);
  const [current, setCurrent] = useState<string | null>(null);
  const focusTab = useFocusTab(box);
  const pop = usePopOut(props.paneRoute, layout, setValue, focused);
  const show = (id: string) => { setCurrent(id); focusTab(id); };
  return (
    <SelectionProvider page={props.page} onMessage={pop.onMessage}>
      <SlotContext.Provider value={pop.slots}>
        <div ref={box} className="workspace" data-look={value.look === 'app' ? undefined : 'mission'}>
          {props.header && <div className="workspace-header">{props.header}</div>}
          <div className="workspace-bar">
            <LayoutTabs value={value} act={layoutActions(setValue, props.defaults)} />
            <AddPane registry={registry} ctx={props.ctx} layout={layout} onChoose={chooser(layout, setRoot, focused, show, pop.slots.reopen)} />
            <LookSetting value={value} setValue={setValue} />
          </div>
          <Notice text={pop.notice} dismiss={pop.dismiss} />
          <Main layout={layout} registry={registry} ctx={props.ctx} current={current} onCurrent={setCurrent} setRoot={setRoot} popOut={pop.popOut} focused={focused} />
        </div>
      </SlotContext.Provider>
    </SelectionProvider>
  );
}

/** A page's dockable workspace. Remounts when `page` changes, so each page keeps its own layouts. */
export function WorkspacePage(props: WorkspacePageProps) {
  return <PageShell key={props.page} {...props} />;
}
