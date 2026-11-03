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
  /** A popped-out window is showing this pane (it opened, reloaded or navigated, or was asked): it belongs in `popped`. */
  | { kind: 'out'; instance: string }
  /** Is a window showing this pane? Its window answers "out". */
  | { kind: 'where'; instance: string }
  /** The main window took this pane back (or dropped it): its window should close. */
  | { kind: 'close'; instance: string }
  /** A popped-out pane's settings changed in its own window. */
  | { kind: 'settings'; instance: string; settings: Record<string, unknown> };

type Listener = (m: ChannelMessage) => void;
type Bus = { channel: string; selection: Selection; select(s: Partial<Selection>): void; post(m: ChannelMessage): void; listen(fn: Listener): () => void };

const BusContext = createContext<Bus | null>(null);
/** False inside a pane whose "Follow selection" setting is off. */
export const FollowContext = createContext(true);

const EMPTY: Selection = {};
const noop = () => {};
const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
const optional = (v: unknown, type: 'string' | 'number') => v === undefined || (typeof v === type && (type === 'string' || Number.isFinite(v)));
const isSelection = (v: unknown): v is Selection => isObj(v) && optional(v.agent, 'string') && optional(v.time, 'number') && optional(v.key, 'string');
const ABOUT_A_PANE = new Set(['back', 'out', 'close', 'where']);

/** Only well-formed messages are heard: another window (or another version of this app) may send anything. */
function isMessage(v: unknown): v is ChannelMessage {
  if (!isObj(v)) return false;
  if (v.kind === 'ask') return true;
  if (v.kind === 'select') return isSelection(v.selection);
  if (typeof v.instance !== 'string') return false;
  return ABOUT_A_PANE.has(v.kind as string) || (v.kind === 'settings' && isObj(v.settings));
}
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
  const bus = useMemo(() => ({ channel: page, selection, select, post, listen }), [page, selection, select, post, listen]);
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

/** The workspace's channel name (the page's, or its own on a shared page); '' outside one. */
export function useChannelName(): string {
  return useContext(BusContext)?.channel ?? '';
}

/** Posts on the workspace's channel (a popped-out window saying it is closing). */
export function useChannelPost(): (m: ChannelMessage) => void {
  return useContext(BusContext)?.post ?? noop;
}
