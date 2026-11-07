import { errorText } from './errors';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api, HISTORY_LIMIT } from './api';
import type { EventRecord } from './generated/EventRecord';
import { FEED_LIMIT } from './useRigStream';
import { isShortened } from './liveText';

export const GAP_NOTICE = 'Some events between history and live updates were not loaded.';

type Loaded = { seatId: string; events: EventRecord[] };

/**
 * The newest HISTORY_LIMIT events from the API plus live stream events newer than
 * them. Re-fetches on each snapshot (epoch). `truncated` means older events exist
 * that were not loaded. History belongs to one seat: switching seats shows loading,
 * never the previous seat's events. If the capped live feed starts after the end of
 * history, events in between may be missing; history is refetched once, and `gap`
 * stays true if the hole persists.
 */
export function useSeatEvents(seatId: string, feed: EventRecord[], epoch: number) {
  const [reloads, setReloads] = useState(0);
  // The history a gap was first seen with; one refetch per gap.
  const refetchedFrom = useRef<Loaded | null>(null);
  const { loaded, failed } = useHistory(seatId, epoch, reloads);

  const history = loaded?.seatId === seatId ? loaded.events : null;
  const error = failed?.seatId === seatId ? failed.message : null;
  const { last, gap } = historyBoundary(history, feed);

  useEffect(() => {
    if (!gap) refetchedFrom.current = null;
    else if (refetchedFrom.current === null && loaded) {
      refetchedFrom.current = loaded;
      setReloads((n) => n + 1);
    }
  }, [gap, loaded]);

  const live = useMemo(() => feed.filter((e) => e.seq > last), [feed, last]);
  const full = useFullCopies(seatId, live);
  const events = useMemo(() => [...(history ?? []), ...live.map((e) => full.get(e.seq) ?? e)], [history, live, full]);

  return {
    events,
    loading: history === null && !error,
    error,
    truncated: (history?.length ?? 0) >= HISTORY_LIMIT,
    // Only a hole that survived the refetch is reported.
    gap: gap && refetchedFrom.current !== null && refetchedFrom.current !== loaded,
  };
}

function historyBoundary(history: EventRecord[] | null, feed: EventRecord[]) {
  const last = history && history.length > 0 ? history[history.length - 1].seq : 0;
  const gap = history !== null && last > 0 && feed.length >= FEED_LIMIT && feed[0].seq > last;
  return { last, gap };
}

/** The seat's latest history page, fetched again on each epoch and reload. */
function useHistory(seatId: string, epoch: number, reloads: number) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [failed, setFailed] = useState<{ seatId: string; message: string } | null>(null);
  useEffect(() => {
    let live = true;
    setFailed(null);
    api
      .latestSeatEvents(seatId)
      .then((h) => live && setLoaded({ seatId, events: h }))
      .catch((e: Error) => live && setFailed({ seatId, message: errorText(e) }));
    return () => {
      live = false;
    };
  }, [seatId, epoch, reloads]);
  return { loaded, failed };
}

const NO_COPIES = new Map<number, EventRecord>();
/** Full copies kept for one agent at most (the newest win). */
const MAX_COPIES = 500;

/**
 * Full copies of the open agent's live events whose long text the live list cut (`liveText`),
 * fetched from the server at most once a second.
 */
function useFullCopies(seatId: string, live: EventRecord[]): Map<number, EventRecord> {
  const [full, setFull] = useState<{ seatId: string; bySeq: Map<number, EventRecord> }>({ seatId, bySeq: NO_COPIES });
  const [retries, setRetries] = useState(0);
  const lastFetch = useRef(0);
  const have = full.seatId === seatId ? full.bySeq : NO_COPIES;
  const from = live.find((e) => isShortened(e) && !have.has(e.seq))?.seq;
  useEffect(() => {
    if (from === undefined) return;
    let current = true;
    const t = setTimeout(() => {
      lastFetch.current = Date.now();
      api
        .seatEventsAfter(seatId, from - 1, 200)
        .then((list) => current && setFull((prev) => ({ seatId, bySeq: withCopies(prev.seatId === seatId ? prev.bySeq : NO_COPIES, list) })))
        .catch(() => current && setTimeout(() => setRetries((n) => n + 1), 3000));
    }, Math.max(0, lastFetch.current + 1000 - Date.now()));
    return () => {
      current = false;
      clearTimeout(t);
    };
  }, [seatId, from, retries]);
  return have;
}

function withCopies(prev: Map<number, EventRecord>, list: EventRecord[]): Map<number, EventRecord> {
  const next = new Map(prev);
  for (const e of list) next.set(e.seq, e);
  for (const k of next.keys()) {
    if (next.size <= MAX_COPIES) break;
    next.delete(k);
  }
  return next;
}
