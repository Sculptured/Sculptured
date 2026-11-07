import { useEffect, useReducer, useState } from 'react';
import type { RigSnapshot } from './generated/RigSnapshot';
import type { SeatView } from './generated/SeatView';
import type { StreamItem } from './generated/StreamItem';
import type { EventRecord } from './generated/EventRecord';
import type { TaggedStreamItem } from './generated/TaggedStreamItem';
import { shortenForFeed } from './liveText';

export type RigState = {
  snapshot: RigSnapshot | null;
  seats: Record<string, SeatView>;
  /** Live events per seat id, oldest first, at most FEED_LIMIT. */
  feed: Record<string, EventRecord[]>;
  /** Increments on every snapshot; history loaders re-fetch when it changes. */
  epoch: number;
  /** Events the server reported lost (the sum of `events-lost` counts) since this view opened. */
  gaps: number;
};

export type StreamMsg =
  | { kind: 'snapshot'; snapshot: RigSnapshot }
  | { kind: 'event'; item: StreamItem }
  /** Drop everything shown for the previous rig. The epoch stays monotonic. */
  | { kind: 'reset' };

/** First error retry delay, doubled per consecutive failure up to BACKOFF_MAX_MS. */
export const BACKOFF_BASE_MS = 1000;
export const BACKOFF_MAX_MS = 30000;
/** Minimum delay before reopening after `resync` or `events-lost`. */
export const RESYNC_MIN_MS = 250;

export const FEED_LIMIT = 500;
export const emptyState: RigState = { snapshot: null, seats: {}, feed: {}, epoch: 0, gaps: 0 };

export function reduce(state: RigState, msg: StreamMsg): RigState {
  if (msg.kind === 'reset') return { ...emptyState, epoch: state.epoch };
  if (msg.kind === 'snapshot') {
    const seats = Object.fromEntries(msg.snapshot.seats.map((s) => [s.id, s]));
    return { ...state, snapshot: msg.snapshot, seats, epoch: state.epoch + 1 };
  }
  const { event } = msg.item;
  if (event.body.type === 'events-lost') return { ...state, gaps: state.gaps + event.body.count };
  // Stop team / Start team change what the team is set to; the snapshot follows.
  if (event.body.type === 'rig-desired-changed') {
    if (!state.snapshot || event.subject.kind !== 'rig' || event.subject.id !== state.snapshot.rig.name) return state;
    return { ...state, snapshot: { ...state.snapshot, desired: event.body.desired } };
  }
  return applySeatEvent(state, msg.item);
}

function applySeatEvent(state: RigState, { event, seat }: StreamItem): RigState {
  if (event.subject.kind !== 'seat') return state;
  const id = event.subject.id;
  const prev = state.feed[id] ?? [];
  // The stream is in seq order. An event this feed already has (a duplicate) carries
  // an older seat view, so it must not replace the one already shown.
  if (prev.length > 0 && prev[prev.length - 1].seq >= event.seq) return state;
  const seats = seat ? { ...state.seats, [seat.id]: seat } : state.seats;
  // Long tool output and briefings are cut in this list; the open agent loads its events in full.
  const next = [...prev, shortenForFeed(event)].slice(-FEED_LIMIT);
  return { ...state, seats, feed: { ...state.feed, [id]: next } };
}

export type EventSourceLike = {
  addEventListener(type: string, listener: (e: MessageEvent) => void): void;
  close(): void;
  onerror: ((e: Event) => void) | null;
};
export type OpenStream = (url: string) => EventSourceLike;

export const openEventSource: OpenStream = (url) => new EventSource(url);

type StreamHandlers = {
  snapshot: (snapshot: RigSnapshot) => void;
  event: (item: StreamItem) => void;
  /** The every-team stream sent all its snapshots. */
  ready?: () => void;
  /** False whenever the connection closes for a reopen. */
  disconnected: () => void;
};

/**
 * Snapshot-first subscription to `url`. On `resync` (server lagged) or `events-lost`
 * (a host gap) it reopens after at least 250 ms; on error or a malformed frame after at
 * least 1 s. Consecutive reopens without a snapshot double the delay up to 30 s; a snapshot
 * resets it. Each reopen starts with fresh snapshots. Returns the cleanup.
 */
function subscribeStream(url: string, openStream: OpenStream, on: StreamHandlers): () => void {
  let es: EventSourceLike | null = null;
  let cancelled = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Delay the next reopen will use at least; 0 right after a snapshot.
  let backoff = 0;

  const reconnect: Reconnect = {
    reopen: (minDelayMs) => {
      es?.close();
      es = null;
      on.disconnected();
      clearTimeout(timer);
      const wait = Math.min(BACKOFF_MAX_MS, Math.max(minDelayMs, backoff));
      backoff = Math.min(BACKOFF_MAX_MS, wait * 2);
      timer = setTimeout(open, wait);
    },
    synced: () => void (backoff = 0),
  };

  function open() {
    if (cancelled) return;
    es = openStream(url);
    listen(es, on, reconnect);
  }

  open();
  return () => {
    cancelled = true;
    clearTimeout(timer);
    es?.close();
  };
}

type Reconnect = { reopen: (minDelayMs: number) => void; synced: () => void };

/** Wires one connection to the handlers; snapshots and `ready` reset the backoff. */
function listen(source: EventSourceLike, on: StreamHandlers, r: Reconnect) {
  // A malformed frame cannot be trusted; start over from a fresh snapshot.
  const parse = <T,>(e: MessageEvent): T | undefined => {
    try {
      return JSON.parse(e.data) as T;
    } catch {
      r.reopen(BACKOFF_BASE_MS);
      return undefined;
    }
  };
  source.addEventListener('snapshot', (e) => {
    const snapshot = parse<RigSnapshot>(e);
    if (!snapshot) return;
    r.synced();
    on.snapshot(snapshot);
  });
  source.addEventListener('ready', () => {
    r.synced();
    on.ready?.();
  });
  source.addEventListener('event', (e) => {
    const item = parse<StreamItem>(e);
    if (!item) return;
    on.event(item);
    if (item.event.body.type === 'events-lost') r.reopen(RESYNC_MIN_MS);
  });
  source.addEventListener('resync', () => r.reopen(RESYNC_MIN_MS));
  source.onerror = () => r.reopen(BACKOFF_BASE_MS);
}

/** Snapshot-first subscription to one rig (see `subscribeStream`). Returns the cleanup. */
export function subscribeRig(
  rig: string,
  openStream: OpenStream,
  dispatch: (msg: StreamMsg) => void,
  setConnected: (c: boolean) => void,
): () => void {
  return subscribeStream(`/api/stream?rig=${encodeURIComponent(rig)}`, openStream, {
    snapshot: (snapshot) => {
      setConnected(true);
      dispatch({ kind: 'snapshot', snapshot });
    },
    event: (item) => dispatch({ kind: 'event', item }),
    disconnected: () => setConnected(false),
  });
}

/** Every team over one connection, so the browser never runs out of connections. */
export const ALL_TEAMS_STREAM = '/api/stream?all=true';

/** The rig an event concerns; null for a host event, which concerns every rig. */
export function rigOfItem(item: StreamItem | TaggedStreamItem): string | null {
  // The every-team stream tags each item; derive it only for an untagged item.
  if ('rig' in item && item.rig !== undefined) return item.rig;
  const s = item.event.subject;
  if (s.kind === 'seat') return s.id.slice(s.id.lastIndexOf('@') + 1);
  if (s.kind === 'rig') return s.id;
  return null;
}

/**
 * One subscription for every team. `dispatch` gets each team's messages (`rig` null: a
 * host event for every team); `setConnected(true)` once all snapshots arrived, false on
 * every reopen. Returns the cleanup.
 */
export function subscribeTeams(
  openStream: OpenStream,
  dispatch: (rig: string | null, msg: StreamMsg) => void,
  setConnected: (c: boolean) => void,
): () => void {
  return subscribeStream(ALL_TEAMS_STREAM, openStream, {
    snapshot: (snapshot) => dispatch(snapshot.rig.name, { kind: 'snapshot', snapshot }),
    event: (item) => dispatch(rigOfItem(item), { kind: 'event', item }),
    ready: () => setConnected(true),
    disconnected: () => setConnected(false),
  });
}

/** One rig's stream state; a rig change clears all state. */
export function useRigStream(rig: string, openStream: OpenStream = openEventSource) {
  const [state, dispatch] = useReducer(reduce, emptyState);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    // A different rig must not show the previous rig's data.
    dispatch({ kind: 'reset' });
    setConnected(false);
    return subscribeRig(rig, openStream, dispatch, setConnected);
  }, [rig, openStream]);

  return { state, connected };
}
