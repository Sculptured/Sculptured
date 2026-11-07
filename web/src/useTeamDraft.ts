import { errorText } from './errors';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api, ApiError, newRequestId } from './api';
import { push, redo as redoStep, start, undo as undoStep, type History } from './draft/history';
import type { DraftIssue } from './generated/DraftIssue';
import type { TeamDraft } from './generated/TeamDraft';
import type { TeamRecord } from './generated/TeamRecord';

/** How often a team waiting for its computer re-checks whether it connected. */
const COMPUTER_RECHECK_MS = 5_000;

export type SaveState = 'saved' | 'unsaved' | 'saving' | 'conflict' | 'error';

export const SAVE_DELAY_MS = 500;
/** Tests shorten the autosave delay through this. */
export const saveTiming = { delayMs: SAVE_DELAY_MS };

/** What an edit function returns: the next draft, or the same draft plus why it was refused. */
export type EditResult = TeamDraft | { draft: TeamDraft; reason?: string };

const unpack = (r: EditResult): { draft: TeamDraft; reason?: string } => ('agents' in r ? { draft: r } : r);

type DraftRef = { draft: TeamDraft; id: string };

/** What the sync keeps between renders, as plain refs created once for the hook's lifetime. */
function makeRefs(teamId: string) {
  return {
    histRef: { current: null as History<TeamDraft> | null },
    revision: { current: 0 },
    sent: { current: null as TeamDraft | null },
    timer: { current: undefined as ReturnType<typeof setTimeout> | undefined },
    inflight: { current: null as Promise<void> | null },
    blocked: { current: false },
    failed: { current: false },
    retryOf: { current: null as DraftRef | null },
    alive: { current: true },
    /** Bumped on every load; a save loop or keepalive answer from an earlier load is ignored. */
    session: { current: 0 },
    /** The team the current load belongs to; saves go to it even after `teamId` changes. */
    savingFor: { current: teamId },
  };
}
type Refs = ReturnType<typeof makeRefs>;

/** The draft's state, and the stable setters the sync reaches it through. */
function useDraftState(r: Refs) {
  const [hist, setHist] = useState<History<TeamDraft> | null>(null);
  const [issues, setIssues] = useState<DraftIssue[]>([]);
  const [serverLive, setServerLive] = useState(false);
  const [startedRevision, setStartedRevision] = useState<number | null>(null);
  const [saveState, setSaveState] = useState<SaveState>('saved');
  const [saveError, setSaveError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [origin, setOrigin] = useState<TeamRecord['origin'] | null>(null);
  const set = useMemo(() => ({ setHist, setIssues, setServerLive, setStartedRevision, setSaveState, setSaveError, setLoadError, setNotice, setOrigin }), []);
  const commit = useCallback((h: History<TeamDraft>) => {
    r.histRef.current = h;
    setHist(h);
  }, [r]);
  const adoptRead = useCallback((rec: TeamRecord) => {
    setIssues(rec.issues);
    setServerLive(rec.live);
    setStartedRevision(rec.started_revision);
    setOrigin(rec.origin ?? null);
  }, []);
  const handles = useMemo(() => ({ set, commit, adoptRead }), [set, commit, adoptRead]);
  return { values: { hist, issues, serverLive, startedRevision, saveState, saveError, loadError, notice, origin }, handles };
}
type Handles = ReturnType<typeof useDraftState>['handles'];
type Set = Handles['set'];

/** The request id for saving `draft`: reused only to retry the very draft whose request may have reached the server. */
function requestFor(r: Refs, draft: TeamDraft) {
  const id = r.retryOf.current?.draft === draft ? r.retryOf.current.id : newRequestId();
  r.retryOf.current = { draft, id };
  return id;
}

/** A save failed: a conflict blocks autosave until Reload or Keep mine; any other failure waits for a retry. */
function saveFailed(r: Refs, set: Set, e: unknown, draft: TeamDraft, id: string) {
  const conflict = e instanceof ApiError && e.code === 'draft_conflict';
  r.retryOf.current = e instanceof ApiError ? null : { draft, id };
  if (conflict) r.blocked.current = true;
  r.failed.current = !conflict;
  if (e instanceof ApiError && e.code === 'team_running') set.setServerLive(true);
  if (!r.alive.current) return;
  set.setSaveState(conflict ? 'conflict' : 'error');
  set.setSaveError(errorText(e));
}

/** One save; says whether the loop should go on (a failure ends it). */
async function saveOnce(r: Refs, set: Set, teamId: string, draft: TeamDraft): Promise<'next' | 'stop'> {
  const id = requestFor(r, draft);
  if (r.alive.current) set.setSaveState('saving');
  try {
    const rec = await api.saveTeam(teamId, r.revision.current, draft, id);
    if (r.retryOf.current?.draft === draft) r.retryOf.current = null;
    r.failed.current = false;
    r.revision.current = rec.draft.revision;
    r.sent.current = draft;
    // After the board closed the loop goes on (a newer edit may wait), without the view.
    if (!r.alive.current) return 'next';
    // Issues describe the saved draft; `live` from a save is never trusted (see above).
    set.setIssues(rec.issues);
    set.setSaveError(null);
    return 'next';
  } catch (e) {
    saveFailed(r, set, e, draft, id);
    return 'stop';
  }
}

/** Saves, serialised, and the timer that starts one after the last edit. */
function useSaves(r: Refs, set: Set, saveDelayMs: number) {
  const runSaves = useCallback((): Promise<void> => {
    if (r.inflight.current) return r.inflight.current;
    // Saves belong to the team loaded when they began; a newer load ends this loop.
    const mine = r.session.current;
    const teamId = r.savingFor.current;
    const loop = (async () => {
      while (!r.blocked.current && r.session.current === mine) {
        const draft = r.histRef.current?.present;
        if (!draft || draft === r.sent.current) break;
        if ((await saveOnce(r, set, teamId, draft)) === 'stop') return;
      }
      if (!r.blocked.current && r.alive.current) set.setSaveState('saved');
    })().finally(() => {
      if (r.inflight.current === loop) r.inflight.current = null;
    });
    r.inflight.current = loop;
    return loop;
  }, [r, set]);
  const schedule = useCallback(() => {
    clearTimeout(r.timer.current);
    if (r.blocked.current) return;
    r.failed.current = false;
    set.setSaveState('unsaved');
    r.timer.current = setTimeout(() => void runSaves(), saveDelayMs);
  }, [r, set, runSaves, saveDelayMs]);
  return { runSaves, schedule };
}

/** Adopt a freshly read team: its draft becomes both what is on screen and what the server has. */
function adoptFresh(r: Refs, h: Handles, rec: TeamRecord) {
  r.revision.current = rec.draft.revision;
  r.sent.current = rec.draft;
  r.blocked.current = false;
  r.failed.current = false;
  h.commit(start(rec.draft));
  h.adoptRead(rec);
  h.set.setSaveState('saved');
}

/** Reads the team when it opens; leaving within the autosave delay saves the last edits at once. */
function useLoad(teamId: string, r: Refs, h: Handles, runSaves: () => Promise<void>) {
  useEffect(() => {
    r.alive.current = true;
    r.session.current += 1;
    r.savingFor.current = teamId;
    // A save loop of the previous team ends on its own; this team's saves start fresh.
    r.inflight.current = null;
    r.retryOf.current = null;
    h.set.setHist(null);
    r.histRef.current = null;
    h.set.setLoadError(null);
    Promise.resolve()
      .then(() => api.getTeam(teamId))
      .then((rec) => { if (r.alive.current) adoptFresh(r, h, rec); })
      .catch((e: Error) => r.alive.current && h.set.setLoadError(errorText(e)));
    return () => {
      r.alive.current = false;
      clearTimeout(r.timer.current);
      // Leaving within the autosave delay must not lose the last edits: save them now.
      if (r.histRef.current && r.histRef.current.present !== r.sent.current && !r.blocked.current) void runSaves();
    };
  }, [teamId, r, h, runSaves]);
}

/**
 * Hosted: while the team waits for the user's computer, re-read its issues every few seconds so
 * "Connect your computer" clears by itself once the computer connects. Only the issues are taken,
 * and only when every edit is saved (a save brings its own issues). Nothing polls otherwise.
 */
function useComputerRecheck(teamId: string, r: Refs, set: Set, issues: DraftIssue[]) {
  const needsComputer = issues.some((i) => i.code === 'computer-not-connected');
  useEffect(() => {
    if (!needsComputer) return;
    const mine = r.session.current;
    const timer = window.setInterval(() => {
      if (r.histRef.current?.present !== r.sent.current || r.inflight.current) return;
      api.getTeam(teamId).then((rec) => {
        if (r.alive.current && r.session.current === mine && r.histRef.current?.present === r.sent.current) set.setIssues(rec.issues);
      }, () => {});
    }, COMPUTER_RECHECK_MS);
    return () => window.clearInterval(timer);
  }, [needsComputer, teamId, r, set]);
}

/**
 * The page is going away (closed, reloaded, another site): a normal request would be
 * cancelled, so the unsaved draft goes with keepalive. If the page stays after all (the
 * back/forward cache), its answer counts as the save; a failure falls back to autosave,
 * which retries with the same request id so the server applies the draft once.
 */
function usePageHideSave(r: Refs, set: Set, schedule: () => void) {
  useEffect(() => {
    const leave = () => {
      const draft = r.histRef.current?.present;
      if (!draft || draft === r.sent.current || r.blocked.current) return;
      clearTimeout(r.timer.current);
      const id = requestFor(r, draft);
      const mine = r.session.current;
      api
        .saveTeam(r.savingFor.current, r.revision.current, draft, id, { keepalive: true })
        .then((rec) => {
          if (r.session.current !== mine) return;
          if (r.retryOf.current?.draft === draft) r.retryOf.current = null;
          r.failed.current = false;
          r.revision.current = Math.max(r.revision.current, rec.draft.revision);
          r.sent.current = draft;
          if (!r.alive.current) return;
          set.setIssues(rec.issues);
          set.setSaveError(null);
          if (!r.inflight.current && r.histRef.current?.present === draft) set.setSaveState('saved');
        })
        .catch(() => {
          if (r.session.current === mine && r.alive.current) schedule();
        });
    };
    window.addEventListener('pagehide', leave);
    return () => window.removeEventListener('pagehide', leave);
  }, [r, set, schedule]);
}

/** Edits, undo and redo; each saves after the delay. */
function useEditing(r: Refs, h0: Handles, schedule: () => void) {
  const { commit } = h0;
  const { setNotice } = h0.set;
  /**
   * Applies `fn` to the current draft. A refused edit (same draft back) adds no history step;
   * its reason is returned and shown. `key` merges consecutive edits (typing, arrow moves).
   */
  const edit = useCallback(
    (fn: (d: TeamDraft) => EditResult, key?: string): string | null => {
      const h = r.histRef.current;
      if (!h) return null;
      const res = unpack(fn(h.present));
      // A move to where the card already is changes nothing: no undo step, no save.
      if (res.draft !== h.present && !res.reason && JSON.stringify(res.draft) === JSON.stringify(h.present)) return null;
      if (res.draft === h.present) {
        setNotice(res.reason ?? null);
        return res.reason ?? null;
      }
      setNotice(null);
      commit(push(h, res.draft, key));
      schedule();
      return null;
    },
    [r, commit, schedule, setNotice],
  );
  const step = useCallback((back: boolean) => {
    const h = r.histRef.current;
    if (!h || (back ? h.past : h.future).length === 0) return;
    setNotice(null);
    commit(back ? undoStep(h) : redoStep(h));
    schedule();
  }, [r, commit, schedule, setNotice]);
  const undo = useCallback(() => step(true), [step]);
  const redo = useCallback(() => step(false), [step]);
  return { edit, undo, redo };
}

/** Flush now, or retry a failed save. */
function useFlushing(r: Refs, runSaves: () => Promise<void>) {
  /** Saves now; resolves once the draft on screen is the draft on the server (or saving failed). */
  const flush = useCallback(async () => {
    clearTimeout(r.timer.current);
    await runSaves();
    if (!r.blocked.current && !r.failed.current && r.histRef.current?.present !== r.sent.current) await runSaves();
    const ok = !r.blocked.current && !r.failed.current && r.histRef.current?.present === r.sent.current;
    return { ok, revision: r.revision.current };
  }, [r, runSaves]);
  const retry = useCallback(async () => {
    r.failed.current = false;
    await runSaves();
  }, [r, runSaves]);
  return { flush, retry };
}

/** Reload the server's draft, keep mine over a conflict, or re-read running state. */
function useReads(teamId: string, r: Refs, h: Handles, runSaves: () => Promise<void>) {
  const { set } = h;
  /** Drops local changes and shows the server's draft. */
  const reload = useCallback(async () => {
    clearTimeout(r.timer.current);
    try {
      const rec = await api.getTeam(teamId);
      r.retryOf.current = null;
      adoptFresh(r, h, rec);
      set.setSaveError(null);
      set.setNotice(null);
    } catch (e) {
      set.setSaveError(errorText(e));
    }
  }, [teamId, r, h, set]);

  /** Conflict: save the local draft over the server's newer revision. */
  const keepMine = useCallback(async () => {
    try {
      const rec = await api.getTeam(teamId);
      r.revision.current = rec.draft.revision;
      r.blocked.current = false;
      r.failed.current = false;
      r.sent.current = null;
      await runSaves();
    } catch (e) {
      set.setSaveError(errorText(e));
    }
  }, [teamId, r, runSaves, set]);

  /** Re-reads running state, issues and started revision (after Start or Stop) without touching the draft. */
  const refresh = useCallback(async (): Promise<TeamRecord | null> => {
    try {
      const rec = await api.getTeam(teamId);
      h.adoptRead(rec);
      return rec;
    } catch (e) {
      set.setSaveError(`Could not check whether this team is running: ${errorText(e)}`);
      return null;
    }
  }, [teamId, h, set]);
  return { reload, keepMine, refresh };
}

/**
 * Loads a team draft and keeps it saved. Every edit goes through the undo history; an edit that
 * was refused (the draft did not change) adds no step and its reason is kept in `notice`.
 * A save runs `saveDelayMs` after the last edit and carries the last revision the server
 * returned. Saves are serialised: an edit made during a save is sent right after it. A 409
 * draft_conflict stops autosave and keeps the local draft on screen until Reload (or Keep mine).
 *
 * `serverLive` comes only from reading the team (load, refresh), never from a save's response:
 * a save can answer while a start is still being claimed.
 */
export function useTeamDraft(teamId: string, saveDelayMs = saveTiming.delayMs) {
  const r = useRef(makeRefs(teamId)).current;
  const d = useDraftState(r);
  const { handles: h } = d;
  const { runSaves, schedule } = useSaves(r, h.set, saveDelayMs);
  useLoad(teamId, r, h, runSaves);
  useComputerRecheck(teamId, r, h.set, d.values.issues);
  usePageHideSave(r, h.set, schedule);
  const editing = useEditing(r, h, schedule);
  const sync = { ...useFlushing(r, runSaves), ...useReads(teamId, r, h, runSaves) };
  const { hist, ...rest } = d.values;
  return {
    draft: hist?.present ?? null,
    history: hist,
    canUndo: (hist?.past.length ?? 0) > 0,
    canRedo: (hist?.future.length ?? 0) > 0,
    ...rest,
    setIssues: h.set.setIssues,
    setNotice: h.set.setNotice,
    ...editing,
    ...sync,
  };
}

export type TeamDraftApi = ReturnType<typeof useTeamDraft>;
