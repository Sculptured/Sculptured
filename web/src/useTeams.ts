import { batcher, STREAM_BATCH } from './batcher';
import { errorText } from './errors';
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { api } from './api';
import type { EventRecord } from './generated/EventRecord';
import type { RigSummary } from './generated/RigSummary';
import { activityOf, type AgentActivity } from './herdr';
import { useMounted, usePolling } from './usePolling';
import { ALL_TEAMS_STREAM, emptyState, openEventSource, reduce, subscribeTeams, type OpenStream, type RigState, type StreamMsg } from './useRigStream';

export type TeamStream = { state: RigState; connected: boolean };
export type Teams = Record<string, TeamStream>;
export { ALL_TEAMS_STREAM };

type All = { connected: boolean; teams: Teams };
type Action = { rig: string | null; msg: StreamMsg } | { connected: boolean } | { batch: Action[] };

function teamsReduce(all: All, a: Action): All {
  // Many events at once: one new state, so one redraw.
  if ('batch' in a) return a.batch.reduce(teamsReduce, all);
  if ('connected' in a) return mapTeams(all, (t) => t.connected === a.connected ? t : { ...t, connected: a.connected }, a.connected);
  // A host event concerns every team (an events-lost gap counts for each).
  if (a.rig === null) {
    return mapTeams(all, (t) => {
      const state = reduce(t.state, a.msg);
      return state === t.state ? t : { ...t, state };
    });
  }
  const prev = all.teams[a.rig] ?? { state: emptyState, connected: all.connected };
  // A team's own snapshot means it is connected, even before the stream is ready.
  const connected = prev.connected || a.msg.kind === 'snapshot';
  return { ...all, teams: { ...all.teams, [a.rig]: { state: reduce(prev.state, a.msg), connected } } };
}

/** Preserve identities when a broadcast leaves every team unchanged. */
function mapTeams(all: All, update: (team: TeamStream) => TeamStream, connected = all.connected): All {
  let changed = all.connected !== connected;
  const teams: Teams = {};
  for (const [rig, team] of Object.entries(all.teams)) {
    teams[rig] = update(team);
    changed ||= teams[rig] !== team;
  }
  return changed ? { connected, teams } : all;
}

/** How often the team list is refreshed, to find teams started elsewhere. */
export const TEAM_LIST_REFRESH_MS = 10_000;

/** Most active first: most needing attention, then most running, then by name. */
export function byActivity(rigs: RigSummary[]): RigSummary[] {
  return [...rigs].sort(
    (a, b) => b.attention_count - a.attention_count || b.running_count - a.running_count || a.name.localeCompare(b.name),
  );
}

const WAITING: TeamStream = { state: emptyState, connected: false };
const READY: TeamStream = { state: emptyState, connected: true };

/**
 * Every team's live state over one connection (`/api/stream?all=true`): a browser allows
 * only a few connections per server, so one per team would stall the page. `names` is the
 * current team, then the rig list, then `also` (teams that have started but may not be in
 * the rig list yet), then any team the stream reported first. A named team the stream has
 * no snapshot for yet reads as an empty state.
 */
export function useTeams(current: string | undefined, openStream: OpenStream = openEventSource, also: string[] = []) {
  const { rigs, listError } = useRigList();
  const [all, dispatch] = useReducer(teamsReduce, { connected: false, teams: {} });

  useEffect(() => {
    const b = batcher<Action>((batch) => dispatch(batch.length === 1 ? batch[0] : { batch }), STREAM_BATCH.ms);
    const stop = subscribeTeams(openStream, (rig, msg) => b.push({ rig, msg }), (connected) => b.push({ connected }));
    return () => {
      stop();
      b.cancel();
    };
  }, [openStream]);

  const alsoKey = also.join('\n');
  const streamKey = Object.keys(all.teams).join('\n');
  const names = useMemo(
    () => teamNames(current, rigs, also, Object.keys(all.teams)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [current, rigs, alsoKey, streamKey],
  );

  const teams = useMemo(() => {
    const out: Teams = {};
    for (const n of names) out[n] = all.teams[n] ?? (all.connected ? READY : WAITING);
    return out;
  }, [names, all]);

  return { rigs, listError, teams, names };
}

/** The rig list, refreshed every TEAM_LIST_REFRESH_MS. */
function useRigList() {
  const [rigs, setRigs] = useState<RigSummary[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const live = useMounted();
  const load = useCallback(
    () =>
      void Promise.resolve()
        .then(() => api.listRigs())
        .then((r) => {
          if (!live.current) return;
          setRigs(Array.isArray(r) ? r : []);
          setListError(null);
        })
        .catch((e: Error) => live.current && setListError(errorText(e))),
    [live],
  );
  usePolling(load, TEAM_LIST_REFRESH_MS);
  return { rigs, listError };
}

/** The current team, then the rig list, then `also`, then teams only the stream reported. */
function teamNames(current: string | undefined, rigs: RigSummary[] | null, also: string[], streamed: string[]): string[] {
  const out = current ? [current] : [];
  for (const n of [...(rigs ?? []).map((r) => r.name), ...also, ...streamed]) if (!out.includes(n)) out.push(n);
  return out;
}

/** Events of the latest turns per agent: live feed plus, for the current team, a short history. */
export const ACTIVITY_TAIL = 60;

/**
 * Tool chips, last agent text and last turn end per agent. The current team's agents load a
 * short history once per snapshot so a fresh page is not empty; other teams use the live feed.
 */
export function useActivity(teams: Teams): Record<string, AgentActivity> {
  const [hist, setHist] = useState<Record<string, EventRecord[]>>({});
  // One short history load per team snapshot (epoch).
  const loaded = useRef(new Map<string, number>());
  const key = Object.entries(teams)
    .map(([rig, t]) => `${rig}:${t.state.epoch}`)
    .join('\n');

  useEffect(() => {
    for (const [rig, t] of Object.entries(teams)) {
      const epoch = t.state.epoch;
      const snap = t.state.snapshot;
      if (!snap || !epoch || loaded.current.get(rig) === epoch) continue;
      loaded.current.set(rig, epoch);
      loadTeamTails(rig, (bySeat) => setHist((h) => ({ ...h, ...bySeat })));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // Per seat, the last inputs and result: a new event recomputes only its own seat.
  const cache = useRef(new Map<string, Cached>());
  const last = useRef<Record<string, AgentActivity>>({});
  return useMemo(() => (last.current = activityBySeat(teams, hist, cache.current, last.current)), [teams, hist]);
}

/** Each of the team's agents' last ACTIVITY_TAIL events about itself, in one request. History is an
 *  addition: a failure is ignored. */
function loadTeamTails(rig: string, onEvents: (bySeat: Record<string, EventRecord[]>) => void) {
  Promise.resolve()
    .then(() => api.teamRecent(rig, ACTIVITY_TAIL))
    .then((bySeat) => {
      if (bySeat && typeof bySeat === 'object') onEvents(bySeat);
    })
    .catch(() => {
      // The live feed still drives the view.
    });
}

type Cached = { feed: EventRecord[]; h: EventRecord[]; activity: AgentActivity };

/**
 * Each seat's activity from its loaded history plus the newer live feed. A seat whose feed and
 * history are the same arrays as last time keeps its result, and when no seat changed the previous
 * object is returned, so what is drawn from it is not redrawn.
 */
export function activityBySeat(teams: Teams, hist: Record<string, EventRecord[]>, cache = new Map<string, Cached>(), previous: Record<string, AgentActivity> = {}): Record<string, AgentActivity> {
  const out: Record<string, AgentActivity> = {};
  let changed = false;
  for (const t of Object.values(teams)) {
    for (const s of t.state.snapshot?.rig.seats ?? []) {
      const feed = t.state.feed[s.id] ?? NONE;
      const h = hist[s.id] ?? NONE;
      const activity = cachedActivity(cache, s.id, feed, h);
      out[s.id] = activity;
      changed ||= previous[s.id] !== activity;
    }
  }
  return changed || Object.keys(previous).length !== Object.keys(out).length ? out : previous;
}

function cachedActivity(cache: Map<string, Cached>, id: string, feed: EventRecord[], h: EventRecord[]): AgentActivity {
  const hit = cache.get(id);
  const activity = hit && hit.feed === feed && hit.h === h ? hit.activity : activityFor(feed, h);
  if (activity !== hit?.activity) cache.set(id, { feed, h, activity });
  return activity;
}

const NONE: EventRecord[] = [];

function activityFor(feed: EventRecord[], h: EventRecord[]): AgentActivity {
  const last = h.length > 0 ? h[h.length - 1].seq : 0;
  return activityOf(h.length > 0 ? [...h, ...feed.filter((e) => e.seq > last)] : feed);
}
