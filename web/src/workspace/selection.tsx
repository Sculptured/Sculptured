import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

/** What the operator last picked: an agent, a moment on the timeline, a world key. */
export type Selection = { agent?: string; time?: number; key?: string };

/** What windows of one workspace say to each other over its BroadcastChannel. */
export type ChannelMessage =
  | { kind: 'select'; selection: Selection }
  /** A window that just opened asks for the current selection. */
  | { kind: 'ask' }
  /** A popped-out window is closing: its pane goes back into the layout. */
  | { kind: 'back'; instance: string }
  /** A popped-out window is showing this pane (it opened, reloaded or navigated): it belongs in `popped`. */
  | { kind: 'out'; instance: string }
  /** The main window took this pane back (or dropped it): its window should close. */
  | { kind: 'close'; instance: string }
  /** A popped-out pane's settings changed in its own window. */
  | { kind: 'settings'; instance: string; settings: Record<string, unknown> };

type Listener = (m: ChannelMessage) => void;
type Bus = { selection: Selection; select(s: Partial<Selection>): void; post(m: ChannelMessage): void; listen(fn: Listener): () => void };

const BusContext = createContext<Bus | null>(null);
/** False inside a pane whose "Follow selection" setting is off. */
export const FollowContext = createContext(true);

const EMPTY: Selection = {};
const noop = () => {};
const KINDS = new Set(['select', 'ask', 'back', 'out', 'close', 'settings']);
const isMessage = (v: unknown): v is ChannelMessage => !!v && typeof v === 'object' && KINDS.has((v as { kind?: unknown }).kind as string);
const isEmpty = (s: Selection) => s.agent === undefined && s.time === undefined && s.key === undefined;

export const channelName = (page: string) => `sculptured.workspace.${page}`;

/** Without BroadcastChannel (old browsers, tests) the selection is shared in this window only. */
function openChannel(page: string): BroadcastChannel | null {
  if (typeof BroadcastChannel === 'undefined') return null;
  try { return new BroadcastChannel(channelName(page)); } catch { return null; }
}

/**
 * The page's channel for this component's life. Returns a stable `post`. Posts made before the
 * channel opens (a child's mount effect runs before this one) are sent once it is open.
 */
function useChannel(page: string, onMessage: (m: ChannelMessage) => void) {
  const handler = useRef(onMessage);
  const channel = useRef<BroadcastChannel | null>(null);
  const early = useRef<ChannelMessage[] | null>([]);
  useEffect(() => { handler.current = onMessage; });
  useEffect(() => {
    const c = openChannel(page);
    channel.current = c;
    if (c) c.onmessage = (e: MessageEvent) => { if (isMessage(e.data)) handler.current(e.data); };
    early.current?.forEach((m) => c?.postMessage(m));
    early.current = null;
    return () => { c?.close(); channel.current = null; };
  }, [page]);
  return useCallback((m: ChannelMessage) => { if (early.current) early.current.push(m); else channel.current?.postMessage(m); }, []);
}

/** Tells the windows on another channel to close these panes (their main window moved on to another channel). */
export function postClose(page: string, instances: string[]): void {
  const c = openChannel(page);
  if (!c) return;
  instances.forEach((instance) => c.postMessage({ kind: 'close', instance } satisfies ChannelMessage));
  c.close();
}

type ProviderProps = { page: string; children: ReactNode };

/**
 * Holds the workspace's selection and mirrors it over `BroadcastChannel('sculptured.workspace.<page>')`,
 * so popped-out windows follow it. Other messages go to useChannelListener.
 */
export function SelectionProvider({ page, children }: ProviderProps) {
  const [selection, setSelection] = useState<Selection>(EMPTY);
  const current = useRef(selection);
  const listeners = useRef(new Set<Listener>());
  const take = (next: Selection) => { current.current = next; setSelection(next); };
  const post = useChannel(page, (m) => {
    if (m.kind === 'select') take(m.selection);
    else if (m.kind === 'ask') { if (!isEmpty(current.current)) post({ kind: 'select', selection: current.current }); }
    else listeners.current.forEach((fn) => fn(m));
  });
  const listen = useCallback((fn: Listener) => { listeners.current.add(fn); return () => void listeners.current.delete(fn); }, []);
  useEffect(() => post({ kind: 'ask' }), [post]);
  const select = useCallback((s: Partial<Selection>) => {
    const next = { ...current.current, ...s };
    current.current = next;
    setSelection(next);
    post({ kind: 'select', selection: next });
  }, [post]);
  const bus = useMemo(() => ({ selection, select, post, listen }), [selection, select, post, listen]);
  return <BusContext.Provider value={bus}>{children}</BusContext.Provider>;
}

/**
 * The shared selection and a way to change it. In a pane with "Follow selection" off, `selection`
 * stays at what the pane last followed; `select` still selects for everyone else.
 */
export function useSelection(): { selection: Selection; select(s: Partial<Selection>): void } {
  const bus = useContext(BusContext);
  const follow = useContext(FollowContext);
  const live = bus?.selection ?? EMPTY;
  const [pinned, setPinned] = useState(live);
  if (follow && pinned !== live) setPinned(live);
  return { selection: follow ? live : pinned, select: bus?.select ?? noop };
}

/** Hears the channel's messages other than the selection (pop-out comings and goings, settings). */
export function useChannelListener(fn: Listener): void {
  const listen = useContext(BusContext)?.listen;
  const latest = useRef(fn);
  useEffect(() => { latest.current = fn; });
  useEffect(() => listen?.((m) => latest.current(m)), [listen]);
}

/** Posts on the workspace's channel (a popped-out window saying it is closing). */
export function useChannelPost(): (m: ChannelMessage) => void {
  return useContext(BusContext)?.post ?? noop;
}
