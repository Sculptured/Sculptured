import { useCallback, useEffect, useMemo, useState } from 'react';
import { ApiError } from '../api';
import { usePaneContext } from '../workspace/registry';
import { arena } from '../tests/api';
import { HEALTH_CHANGED, useArenaHealth } from '../tests/shared';
import type { Health, RunSummary, UsageResponse, WorldResponse } from '../tests/types';

/** The last answer (kept when a later read fails) and the last read's error. */
export type Loaded<T> = { value: T | null; error: unknown };

/** A cleanup run from this browser: when, and what it freed. Arena's own startup cleanup is not known here. */
export type Cleanup = { at: number; freed_bytes: number };

/** What every Arena control pane and the status strip share. */
export type ArenaCtx = {
  /** Undefined while asking, null when Arena is not answering (see `healthError`). */
  health: Health | null | undefined;
  healthError: unknown;
  runs: Loaded<RunSummary[]>;
  world: Loaded<WorldResponse>;
  usage: Loaded<UsageResponse>;
  lastCleanup: Cleanup | null;
  refreshRuns(): void;
  /** After a cleanup: remember it, then re-read health (and with it usage and runs). */
  cleanedUp(c: Cleanup): void;
};

/** Arena's own "not answering": controld could not reach arenad. */
export const isArenaDown = (e: unknown) => e instanceof ApiError && e.code === 'arena_unreachable';

const CLEANUP_KEY = 'sculptured.arena.cleanup.v1';

function readCleanup(): Cleanup | null {
  try {
    const v = JSON.parse(localStorage.getItem(CLEANUP_KEY) ?? 'null') as Cleanup | null;
    return v && typeof v.at === 'number' && typeof v.freed_bytes === 'number' ? v : null;
  } catch { return null; }
}

/** Reads `load` whenever `tick` changes while `on`; a failed read keeps the last answer and says why. */
function useLoaded<T>(load: () => Promise<T>, tick: unknown, on: boolean) {
  const [state, setState] = useState<Loaded<T>>({ value: null, error: null });
  const reload = useCallback(() => {
    load().then((value) => setState({ value, error: null })).catch((error) => setState((s) => ({ value: s.value, error })));
  }, [load]);
  useEffect(() => { if (on) reload(); }, [tick, on, reload]);
  return { state, reload };
}

/** Every 3 s while a branch of some run is running. */
function usePollWhileRunning(runs: RunSummary[] | null, reload: () => void) {
  const running = !!runs?.some((r) => r.branches.some((b) => b.status === 'running'));
  useEffect(() => {
    if (!running) return;
    const t = setInterval(reload, 3000);
    return () => clearInterval(t);
  }, [running, reload]);
}

/**
 * The page's context for its panes (the main window and each popped-out one read their own). Runs,
 * usage and the world are read on every health check (15 s, and after a delete or cleanup).
 */
export function useArenaData(): ArenaCtx {
  const { health, error: healthError } = useArenaHealth();
  const up = !!health;
  const runs = useLoaded(arena.runs, health, up);
  const usage = useLoaded(arena.usage, health, up);
  const world = useLoaded(arena.world, health, up);
  usePollWhileRunning(runs.state.value, runs.reload);
  const [lastCleanup, setLastCleanup] = useState(readCleanup);
  const cleanedUp = useCallback((c: Cleanup) => {
    try { localStorage.setItem(CLEANUP_KEY, JSON.stringify(c)); } catch { /* kept for this page only */ }
    setLastCleanup(c);
    window.dispatchEvent(new Event(HEALTH_CHANGED));
  }, []);
  return useMemo(() => ({
    health, healthError, runs: runs.state, world: world.state, usage: usage.state, lastCleanup, refreshRuns: runs.reload, cleanedUp,
  }), [health, healthError, runs.state, world.state, usage.state, lastCleanup, runs.reload, cleanedUp]);
}

export const useArena = () => usePaneContext<ArenaCtx>();
