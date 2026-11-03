import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { closePane, duplicatePane, freshId, movePane, panes, resize, setActive, setPaneSettings, stacks, type Node, type PaneRef, type Split } from './model';
import type { MoveTarget } from './PaneMenu';
import { PaneContext, type Registry } from './registry';
import { Splitter } from './Splitter';
import { nodeTitle, PaneBody, StackView, titleOf, WorkspaceContext, type Actions, type WorkspaceApi } from './StackView';
import { useDrag } from './useDrag';

export type WorkspaceProps = {
  root: Node;
  registry: Registry;
  /** Page-specific context, live: panes read it with usePaneContext. */
  ctx?: unknown;
  onChange(root: Node): void;
  onPopOut?(paneId: string): void;
};

type Live = { root: Node; seen: Node; onChange(root: Node): void; onPopOut?(paneId: string): void };

/** Where focus goes once a pane leaves: a tab beside it, else the nearest other stack's active tab. */
type FocusWant = { tab: string } | { more: string };

function focusAfterClose(root: Node, paneId: string): FocusWant | null {
  const all = stacks(root);
  const at = all.findIndex((s) => s.panes.some((p) => p.id === paneId));
  const own = all[at];
  if (!own) return null;
  const i = own.panes.findIndex((p) => p.id === paneId);
  const beside = own.panes[i + 1] ?? own.panes[i - 1];
  if (beside) return { tab: beside.id };
  const other = all[at - 1] ?? all[at + 1];
  return other?.panes.length ? { tab: other.panes[other.active].id } : null;
}

function stackOf(root: Node, paneId: string): string | undefined {
  return stacks(root).find((s) => s.panes.some((p) => p.id === paneId))?.id;
}

function findSplit(node: Node, id: string): Split | undefined {
  if (node.kind === 'stack') return undefined;
  if (node.id === id) return node;
  for (const kid of node.kids) {
    const found = findSplit(kid, id);
    if (found) return found;
  }
  return undefined;
}

/** Other stacks by their active pane's title (numbered when two share one), then this stack's own edges. */
function moveTargets(root: Node, registry: Registry, stackId: string): MoveTarget[] {
  const all = stacks(root);
  const count = new Map<string, number>();
  const others = all.filter((s) => s.id !== stackId && s.panes.length).map((s) => {
    const title = titleOf(registry, s.panes[s.active]);
    const n = (count.get(title) ?? 0) + 1;
    count.set(title, n);
    return { stackId: s.id, title: n > 1 ? `${title} ${n}` : title };
  });
  const own = all.find((s) => s.id === stackId);
  return own && own.panes.length > 1 ? [...others, { stackId, title: 'this group', own: true }] : others;
}

/**
 * The latest layout, ahead of props between a change and the parent's re-render, so quick
 * successive edits (a splitter drag) build on each other. A new `root` prop always wins.
 */
function useLive(props: WorkspaceProps): RefObject<Live> {
  const live = useRef<Live>({ root: props.root, seen: props.root, onChange: props.onChange, onPopOut: props.onPopOut });
  useLayoutEffect(() => {
    const l = live.current;
    if (l.seen !== props.root) Object.assign(l, { root: props.root, seen: props.root });
    Object.assign(l, { onChange: props.onChange, onPopOut: props.onPopOut });
  });
  return live;
}

function useActions(live: RefObject<Live>, focus: (want: FocusWant | null) => void): Actions {
  return useMemo(() => {
    const now = () => live.current.root;
    const apply = (next: Node) => {
      if (next === now()) return;
      live.current.root = next;
      live.current.onChange(next);
    };
    return {
      activate: (stackId, index) => apply(setActive(now(), stackId, index)),
      close: (paneId) => { const want = focusAfterClose(now(), paneId); apply(closePane(now(), paneId)); focus(want); },
      duplicate: (paneId) => { const id = freshId(); apply(duplicatePane(now(), paneId, id)); focus({ tab: id }); },
      move: (paneId, t) => { apply(movePane(now(), paneId, t.stackId, t.zone)); focus({ tab: paneId }); },
      setSettings: (paneId, settings) => apply(setPaneSettings(now(), paneId, settings)),
      resize: (splitId, index, delta) => apply(resize(now(), splitId, index, delta)),
      sizeOf: (splitId, index) => findSplit(now(), splitId)?.sizes[index] ?? 0,
      popOut: (paneId) => { const stackId = stackOf(now(), paneId); live.current.onPopOut?.(paneId); if (stackId) focus({ more: stackId }); },
    };
  }, [live, focus]);
}

const scrolled = new WeakMap<HTMLElement, { top: number; left: number }>();

function makeHost(paneId: string): HTMLDivElement {
  const host = document.createElement('div');
  host.className = 'ws-pane';
  host.id = `ws-panel-${paneId}`;
  host.setAttribute('role', 'tabpanel');
  host.setAttribute('aria-labelledby', `ws-tab-${paneId}`);
  scrolled.set(host, { top: 0, left: 0 });
  host.addEventListener('scroll', () => scrolled.set(host, { top: host.scrollTop, left: host.scrollLeft }), { passive: true });
  return host;
}

/**
 * One element per pane id, kept for the pane's life. Bodies render into it through a portal, and
 * stacks move the element into their slot, so a pane survives any rearranging without a remount.
 * A browser forgets the scroll of a moved element, so it is put back.
 */
function usePaneHosts(root: Node) {
  const hosts = useRef(new Map<string, HTMLDivElement>());
  const hostFor = useCallback((id: string) => {
    const host = hosts.current.get(id) ?? makeHost(id);
    hosts.current.set(id, host);
    return host;
  }, []);
  const place = useCallback((slot: HTMLElement, list: PaneRef[], active: number) => {
    list.forEach((p, i) => {
      const host = hostFor(p.id);
      if (host.parentElement !== slot) {
        const at = scrolled.get(host);
        slot.append(host);
        if (at) Object.assign(host, { scrollTop: at.top, scrollLeft: at.left });
      }
      host.classList.toggle('is-on', i === active);
      host.toggleAttribute('inert', i !== active);
    });
  }, [hostFor]);
  useLayoutEffect(() => {
    const live = new Set(panes(root).map((p) => p.id));
    for (const [id, host] of hosts.current) if (!live.has(id)) { host.remove(); hosts.current.delete(id); }
  }, [root]);
  return { hostFor, place };
}

/**
 * After a keyboard move or a duplicate, focus follows the pane to its tab. After a close it goes
 * to a neighbouring tab, and after a pop out to the stack's ⋯ button, so it never drops to the
 * page. A target that is gone by then falls back to the first tab still on screen.
 */
function useFocusAfter(el: RefObject<HTMLElement | null>, root: Node) {
  const [want, setWant] = useState<FocusWant | null>(null);
  useEffect(() => {
    if (!want) return;
    const q = 'tab' in want ? `[data-tab="${CSS.escape(want.tab)}"]` : `[data-stack="${CSS.escape(want.more)}"] .ws-more`;
    (el.current?.querySelector<HTMLElement>(q) ?? el.current?.querySelector<HTMLElement>('[role="tab"]'))?.focus();
    setWant(null);
  }, [el, want, root]);
  return setWant;
}

function SplitView({ split, api }: { split: Split; api: WorkspaceApi }) {
  return (
    <div className={`ws-split ws-split-${split.dir}`}>
      {split.kids.map((kid, i) => (
        <Fragment key={kid.id}>
          {i > 0 && (
            <Splitter
              dir={split.dir}
              size={split.sizes[i - 1]}
              label={`Resize ${nodeTitle(api.registry, split.kids[i - 1])} and ${nodeTitle(api.registry, kid)}`}
              current={() => api.act.sizeOf(split.id, i - 1)}
              onResize={(delta) => api.act.resize(split.id, i - 1, delta)}
            />
          )}
          <div className="ws-cell" style={{ flex: `${split.sizes[i]} 1 0` }}>
            <NodeView node={kid} api={api} />
          </div>
        </Fragment>
      ))}
    </div>
  );
}

function NodeView({ node, api }: { node: Node; api: WorkspaceApi }) {
  return node.kind === 'stack' ? <StackView stack={node} /> : <SplitView split={node} api={api} />;
}

/**
 * The docking workspace: drag panes by their tab to split or join, resize, close, duplicate and configure them.
 *
 * Controlled, and the parent must accept every `onChange`: quick successive edits build on the
 * last change before the parent re-renders, so a vetoed change would leave this view ahead of
 * `root` until the next new `root` arrives. WorkspacePage always accepts.
 */
export function Workspace(props: WorkspaceProps) {
  const { root, registry } = props;
  const el = useRef<HTMLDivElement>(null);
  const live = useLive(props);
  const act = useActions(live, useFocusAfter(el, root));
  const { hostFor, place } = usePaneHosts(root);
  const { drag, start } = useDrag(el, {
    noop: (paneId, t) => movePane(live.current.root, paneId, t.stackId, t.zone) === live.current.root,
    drop: act.move,
  });
  const api: WorkspaceApi = { registry, act, drag, startDrag: start, place, targets: (id) => moveTargets(root, registry, id), canPopOut: !!props.onPopOut };
  return (
    <PaneContext.Provider value={props.ctx}>
      <WorkspaceContext.Provider value={api}>
        <div ref={el} className={`ws${drag ? ' is-dragging' : ''}`}>
          <NodeView node={root} api={api} />
        </div>
        {panes(root).map((p) => createPortal(<PaneBody pane={p} type={registry.get(p.type)} act={act} />, hostFor(p.id), p.id))}
      </WorkspaceContext.Provider>
    </PaneContext.Provider>
  );
}
