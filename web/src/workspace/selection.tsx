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
  /** A popped-out pane's settings changed in its own window. */
  | { kind: 'settings'; instance: string; settings: Record<string, unknown> };

type Bus = { selection: Selection; select(s: Partial<Selection>): void; post(m: ChannelMessage): void };

const BusContext = createContext<Bus | null>(null);
/** False inside a pane whose "Follow selection" setting is off. */
export const FollowContext = createContext(true);

const EMPTY: Selection = {};
const noop = () => {};
const KINDS = new Set(['select', 'ask', 'back', 'settings']);
const isMessage = (v: unknown): v is ChannelMessage => !!v && typeof v === 'object' && KINDS.has((v as { kind?: unknown }).kind as string);
const isEmpty = (s: Selection) => s.agent === undefined && s.time === undefined && s.key === undefined;

export const channelName = (page: string) => `sculptured.workspace.${page}`;

/** Without BroadcastChannel (old browsers, tests) the selection is shared in this window only. */
function openChannel(page: string): BroadcastChannel | null {
  if (typeof BroadcastChannel === 'undefined') return null;
  try { return new BroadcastChannel(channelName(page)); } catch { return null; }
}

/** The page's channel for this component's life. Returns a stable `post`. */
function useChannel(page: string, onMessage: (m: ChannelMessage) => void) {
  const handler = useRef(onMessage);
  const channel = useRef<BroadcastChannel | null>(null);
  useEffect(() => { handler.current = onMessage; });
  useEffect(() => {
    const c = openChannel(page);
    channel.current = c;
    if (c) c.onmessage = (e: MessageEvent) => { if (isMessage(e.data)) handler.current(e.data); };
    return () => { c?.close(); channel.current = null; };
  }, [page]);
  return useCallback((m: ChannelMessage) => channel.current?.postMessage(m), []);
}

type ProviderProps = { page: string; onMessage?(m: ChannelMessage): void; children: ReactNode };

/**
 * Holds the workspace's selection and mirrors it over `BroadcastChannel('sculptured.workspace.<page>')`,
 * so popped-out windows follow it. Messages other than the selection go to `onMessage`.
 */
export function SelectionProvider({ page, onMessage, children }: ProviderProps) {
  const [selection, setSelection] = useState<Selection>(EMPTY);
  const current = useRef(selection);
  const take = (next: Selection) => { current.current = next; setSelection(next); };
  const post = useChannel(page, (m) => {
    if (m.kind === 'select') take(m.selection);
    else if (m.kind === 'ask') { if (!isEmpty(current.current)) post({ kind: 'select', selection: current.current }); }
    else onMessage?.(m);
  });
  useEffect(() => post({ kind: 'ask' }), [post]);
  const select = useCallback((s: Partial<Selection>) => {
    const next = { ...current.current, ...s };
    current.current = next;
    setSelection(next);
    post({ kind: 'select', selection: next });
  }, [post]);
  const bus = useMemo(() => ({ selection, select, post }), [selection, select, post]);
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

/** Posts on the workspace's channel (a popped-out window saying it is closing). */
export function useChannelPost(): (m: ChannelMessage) => void {
  return useContext(BusContext)?.post ?? noop;
}
