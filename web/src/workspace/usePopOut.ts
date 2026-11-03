import { useRef, useState, type Dispatch, type RefObject, type SetStateAction } from 'react';
import { applySettings, bringBack, eachLayout, orphans, popOutPane } from './layouts';
import { panes, type Layout, type Node, type PageLayouts } from './model';
import { useChannelListener, useChannelName, useChannelPost } from './selection';
import { slotFor, type SlotActions } from './slots';

/*
 * Who has a pane is decided over the workspace's channel, never by window handles (a reload loses
 * them): a pop-out says "out" whenever it shows a pane and "back" when it goes away, and the main
 * window says "close" when it takes a pane back or drops it.
 */

type SetValue = Dispatch<SetStateAction<PageLayouts>>;

const BLOCKED = 'The browser blocked the new window. Allow pop-ups for this page, then choose Pop out again.';
const FEATURES = 'popup,width=760,height=560';
/** One window per pane of each page and channel (two runs' panes of the same id never share one). */
const windowName = (channel: string, instance: string) => `sculptured-pane-${channel}-${instance}`;

/** Opens a pane's window, or shows the one already open without reloading it. */
function useWindows(paneRoute: (instance: string) => string) {
  const channel = useChannelName();
  const name = (instance: string) => windowName(channel, instance);
  const [notice, setNotice] = useState<string | null>(null);
  const handles = useRef(new Map<string, Window>());
  const done = (instance: string, win: Window | null) => {
    if (win) handles.current.set(instance, win);
    setNotice(win ? null : BLOCKED);
    return !!win;
  };
  const open = (instance: string) => done(instance, window.open(paneRoute(instance), name(instance), FEATURES));
  const show = (instance: string) => {
    const known = handles.current.get(instance);
    if (known && !known.closed) { known.focus(); return true; }
    // No handle (this page was reloaded): naming the window without a URL shows an open one as it is.
    const win = window.open('', name(instance), FEATURES);
    try { if (win?.location.href === 'about:blank') win.location.replace(paneRoute(instance)); } catch { /* Unreadable: it is already the pane's page. */ }
    return done(instance, win);
  };
  return { open, show, notice, dismiss: () => setNotice(null) };
}

/** Pop out, Bring back, and the channel messages that keep `popped` true to what the windows show. */
export function usePopOut(paneRoute: (instance: string) => string, layout: Layout, setValue: SetValue, focused: RefObject<string | undefined>) {
  const post = useChannelPost();
  const win = useWindows(paneRoute);
  const update = (fn: (l: Layout) => Layout) => setValue((v) => eachLayout(v, fn));
  const back = (id: string) => update((l) => bringBack(l, id, focused.current));
  // The pane is taken back first; only then is its window told to close.
  const slots: SlotActions = { bringBack: (id) => { back(id); post({ kind: 'close', instance: id }); }, reopen: (id) => void win.show(id) };
  const popOut = (paneId: string) => {
    const pane = panes(layout.root).find((p) => p.id === paneId);
    const slot = pane && slotFor(pane);
    if (!pane || slot !== undefined) return void (slot && win.show(slot));
    if (win.open(paneId)) update((l) => (l.id === layout.id ? popOutPane(l, paneId) : l));
  };
  useChannelListener((m) => {
    if (m.kind === 'back') back(m.instance);
    else if (m.kind === 'out') update((l) => popOutPane(l, m.instance));
    else if (m.kind === 'settings') update((l) => applySettings(l, m.instance, m.settings));
  });
  return { popOut, slots, notice: win.notice, dismiss: win.dismiss };
}

/**
 * Sets the active layout's tree. A popped pane whose placeholder tab was closed comes back into
 * the layout and its window is told to close, so no pane is left in `popped` without a slot.
 */
export function useSetRoot(layout: Layout, setValue: SetValue, focused: RefObject<string | undefined>) {
  const post = useChannelPost();
  return (root: Node) => {
    const adopt = (l: Layout) => orphans(l).reduce((x, id) => bringBack(x, id, focused.current), l);
    setValue((v) => eachLayout(v, (l) => (l.id === layout.id ? adopt({ ...l, root }) : l)));
    orphans({ ...layout, root }).forEach((instance) => post({ kind: 'close', instance }));
  };
}

/** Tells the windows of popped panes that a layout is about to drop (Reset, Delete) to close. */
export function closeWindowsOf(post: (m: { kind: 'close'; instance: string }) => void, layout: Layout | undefined) {
  layout?.popped.forEach((p) => post({ kind: 'close', instance: p.id }));
}
