import { errorText } from './errors';
import { useCallback, useEffect, useRef, useState } from 'react';
import { api, ApiError, newRequestId } from './api';
import { fitGroups, groupsFit } from './draft/edits';
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
  const [hist, setHist] = useState<History<TeamDraft> | null>(null);
  const [issues, setIssues] = useState<DraftIssue[]>([]);
  const [serverLive, setServerLive] = useState(false);
  const [startedRevision, setStartedRevision] = useState<number | null>(null);
  const [saveState, setSaveState] = useState<SaveState>('saved');
  const [saveError, setSaveError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const histRef = useRef<History<TeamDraft> | null>(null);
  const revision = useRef(0);
  const sent = useRef<TeamDraft | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const inflight = useRef<Promise<void> | null>(null);
  const blocked = useRef(false);
  const failed = useRef(false);
  const retryOf = useRef<{ draft: TeamDraft; id: string } | null>(null);
  const alive = useRef(true);
  /** Bumped on every load; a save loop or keepalive answer from an earlier load is ignored. */
  const session = useRef(0);
  /** The team the current load belongs to; saves go to it even after `teamId` changes. */
  const savingFor = useRef(teamId);

  const commit = useCallback((h: History<TeamDraft>) => {
    histRef.current = h;
    setHist(h);
  }, []);
  const [origin, setOrigin] = useState<TeamRecord['origin'] | null>(null);
  const adoptRead = useCallback((rec: TeamRecord) => {
    setIssues(rec.issues);
    setServerLive(rec.live);
    setStartedRevision(rec.started_revision);
    setOrigin(rec.origin ?? null);
  }, []);

  const runSaves = useCallback((): Promise<void> => {
    if (inflight.current) return inflight.current;
    // Saves belong to the team loaded when they began; a newer load ends this loop.
    const mine = session.current;
    const teamId = savingFor.current;
    const loop = (async () => {
      while (!blocked.current && session.current === mine) {
        const draft = histRef.current?.present;
        if (!draft || draft === sent.current) break;
        // Reuse the id only to retry the very draft whose request may have reached the server.
        const id = retryOf.current?.draft === draft ? retryOf.current.id : newRequestId();
        retryOf.current = { draft, id };
        if (alive.current) setSaveState('saving');
        try {
          const rec = await api.saveTeam(teamId, revision.current, draft, id);
          if (retryOf.current?.draft === draft) retryOf.current = null;
          failed.current = false;
          revision.current = rec.draft.revision;
          sent.current = draft;
          // After the board closed the loop goes on (a newer edit may wait), without the view.
          if (!alive.current) continue;
          // Issues describe the saved draft; `live` from a save is never trusted (see above).
          setIssues(rec.issues);
          setSaveError(null);
        } catch (e) {
          const conflict = e instanceof ApiError && e.code === 'draft_conflict';
          retryOf.current = e instanceof ApiError ? null : { draft, id };
          if (conflict) blocked.current = true;
          failed.current = !conflict;
          if (e instanceof ApiError && e.code === 'team_running') setServerLive(true);
          if (!alive.current) return;
          setSaveState(conflict ? 'conflict' : 'error');
          setSaveError(errorText(e));
          return;
        }
      }
      if (!blocked.current && alive.current) setSaveState('saved');
    })().finally(() => {
      if (inflight.current === loop) inflight.current = null;
    });
    inflight.current = loop;
    return loop;
  }, []);

  const schedule = useCallback(() => {
    clearTimeout(timer.current);
    if (blocked.current) return;
    failed.current = false;
    setSaveState('unsaved');
    timer.current = setTimeout(() => void runSaves(), saveDelayMs);
  }, [runSaves, saveDelayMs]);

  /**
   * Shows a draft read from the server as the present one, with a fresh history. A team imported
   * through the API keeps its file's box sizes: the board shows it fitted, but that is display
   * only. The fitted draft is the baseline (nothing is saved because of it), and the first real
   * edit saves it whole through the normal path. `revision` stays the server's.
   */
  const adoptServerDraft = useCallback(
    (server: TeamDraft) => {
      revision.current = server.revision;
      const shown = groupsFit(server) ? server : fitGroups(server);
      sent.current = shown;
      commit(start(shown));
    },
    [commit],
  );

  useEffect(() => {
    alive.current = true;
    session.current += 1;
    savingFor.current = teamId;
    // A save loop of the previous team ends on its own; this team's saves start fresh.
    inflight.current = null;
    retryOf.current = null;
    setHist(null);
    histRef.current = null;
    setLoadError(null);
    Promise.resolve()
      .then(() => api.getTeam(teamId))
      .then((rec) => {
        if (!alive.current) return;
        adoptServerDraft(rec.draft);
        adoptRead(rec);
        setSaveState('saved');
      })
      .catch((e: Error) => alive.current && setLoadError(errorText(e)));
    return () => {
      alive.current = false;
      clearTimeout(timer.current);
      // Leaving within the autosave delay must not lose the last edits: save them now.
      if (histRef.current && histRef.current.present !== sent.current && !blocked.current) void runSaves();
    };
  }, [teamId, adoptServerDraft, adoptRead, runSaves]);

  // Hosted: while the team waits for the user's computer, re-read its issues every few seconds so
  // "Connect your computer" clears by itself once the computer connects. Only the issues are taken,
  // and only when every edit is saved (a save brings its own issues). Nothing polls otherwise.
  const needsComputer = issues.some((i) => i.code === 'computer-not-connected');
  useEffect(() => {
    if (!needsComputer) return;
    const mine = session.current;
    const timer = window.setInterval(() => {
      if (histRef.current?.present !== sent.current || inflight.current) return;
      api.getTeam(teamId).then((rec) => {
        if (alive.current && session.current === mine && histRef.current?.present === sent.current) setIssues(rec.issues);
      }, () => {});
    }, COMPUTER_RECHECK_MS);
    return () => window.clearInterval(timer);
  }, [needsComputer, teamId]);

  // The page is going away (closed, reloaded, another site): a normal request would be
  // cancelled, so the unsaved draft goes with keepalive. If the page stays after all (the
  // back/forward cache), its answer counts as the save; a failure falls back to autosave,
  // which retries with the same request id so the server applies the draft once.
  useEffect(() => {
    const leave = () => {
      const draft = histRef.current?.present;
      if (!draft || draft === sent.current || blocked.current) return;
      clearTimeout(timer.current);
      const id = retryOf.current?.draft === draft ? retryOf.current.id : newRequestId();
      retryOf.current = { draft, id };
      const mine = session.current;
      api
        .saveTeam(savingFor.current, revision.current, draft, id, { keepalive: true })
        .then((rec) => {
          if (session.current !== mine) return;
          if (retryOf.current?.draft === draft) retryOf.current = null;
          failed.current = false;
          revision.current = Math.max(revision.current, rec.draft.revision);
          sent.current = draft;
          if (!alive.current) return;
          setIssues(rec.issues);
          setSaveError(null);
          if (!inflight.current && histRef.current?.present === draft) setSaveState('saved');
        })
        .catch(() => {
          if (session.current === mine && alive.current) schedule();
        });
    };
    window.addEventListener('pagehide', leave);
    return () => window.removeEventListener('pagehide', leave);
  }, [schedule]);

  /**
   * Applies `fn` to the current draft. A refused edit (same draft back) adds no history step;
   * its reason is returned and shown. `key` merges consecutive edits (typing, arrow moves).
   */
  const edit = useCallback(
    (fn: (d: TeamDraft) => EditResult, key?: string): string | null => {
      const h = histRef.current;
      if (!h) return null;
      const r = unpack(fn(h.present));
      // A move to where the card already is changes nothing: no undo step, no save.
      if (r.draft !== h.present && !r.reason && JSON.stringify(r.draft) === JSON.stringify(h.present)) return null;
      if (r.draft === h.present) {
        setNotice(r.reason ?? null);
        return r.reason ?? null;
      }
      setNotice(null);
      commit(push(h, r.draft, key));
      schedule();
      return null;
    },
    [commit, schedule],
  );

  const undo = useCallback(() => {
    const h = histRef.current;
    if (!h || h.past.length === 0) return;
    setNotice(null);
    commit(undoStep(h));
    schedule();
  }, [commit, schedule]);

  const redo = useCallback(() => {
    const h = histRef.current;
    if (!h || h.future.length === 0) return;
    setNotice(null);
    commit(redoStep(h));
    schedule();
  }, [commit, schedule]);

  /** Saves now; resolves once the draft on screen is the draft on the server (or saving failed). */
  const flush = useCallback(async () => {
    clearTimeout(timer.current);
    await runSaves();
    if (!blocked.current && !failed.current && histRef.current?.present !== sent.current) await runSaves();
    const ok = !blocked.current && !failed.current && histRef.current?.present === sent.current;
    return { ok, revision: revision.current };
  }, [runSaves]);

  const retry = useCallback(async () => {
    failed.current = false;
    await runSaves();
  }, [runSaves]);

  /** Drops local changes and shows the server's draft. */
  const reload = useCallback(async () => {
    clearTimeout(timer.current);
    try {
      const rec = await api.getTeam(teamId);
      blocked.current = false;
      failed.current = false;
      retryOf.current = null;
      adoptServerDraft(rec.draft);
      adoptRead(rec);
      setSaveState('saved');
      setSaveError(null);
      setNotice(null);
    } catch (e) {
      setSaveError(errorText(e));
    }
  }, [teamId, adoptServerDraft, adoptRead]);

  /** Conflict: save the local draft over the server's newer revision. */
  const keepMine = useCallback(async () => {
    try {
      const rec = await api.getTeam(teamId);
      revision.current = rec.draft.revision;
      blocked.current = false;
      failed.current = false;
      sent.current = null;
      await runSaves();
    } catch (e) {
      setSaveError(errorText(e));
    }
  }, [teamId, runSaves]);

  /** Re-reads running state, issues and started revision (after Start or Stop) without touching the draft. */
  const refresh = useCallback(async (): Promise<TeamRecord | null> => {
    try {
      const rec = await api.getTeam(teamId);
      adoptRead(rec);
      return rec;
    } catch (e) {
      setSaveError(`Could not check whether this team is running: ${errorText(e)}`);
      return null;
    }
  }, [teamId, adoptRead]);

  return {
    draft: hist?.present ?? null,
    history: hist,
    canUndo: (hist?.past.length ?? 0) > 0,
    canRedo: (hist?.future.length ?? 0) > 0,
    issues,
    setIssues,
    serverLive,
    startedRevision,
    origin,
    saveState,
    saveError,
    loadError,
    notice,
    setNotice,
    edit,
    undo,
    redo,
    flush,
    retry,
    reload,
    keepMine,
    refresh,
  };
}

export type TeamDraftApi = ReturnType<typeof useTeamDraft>;
