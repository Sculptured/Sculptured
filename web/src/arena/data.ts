import { useCallback, useEffect, useMemo, useState } from 'react';
import { ApiError } from '../api';
import { usePaneContext } from '../workspace/registry';
import { arena } from '../tests/api';
import { useArenaHealth } from '../tests/shared';
import type { Health, RunSummary } from '../tests/types';

/** What every Arena control pane shares: Arena's status and the runs so far. */
export type ArenaCtx = {
  /** Undefined while asking, null when Arena is not answering (see `healthError`). */
  health: Health | null | undefined;
  healthError: unknown;
  /** Null until the first answer. */
  runs: RunSummary[] | null;
  runsError: unknown;
  refreshRuns(): void;
};

/** Arena's own "not answering": controld could not reach arenad. */
export const isArenaDown = (e: unknown) => e instanceof ApiError && e.code === 'arena_unreachable';

/** The runs, read once Arena answers and every 3 s while a branch is running. */
function useRuns(up: boolean) {
  const [runs, setRuns] = useState<RunSummary[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const refresh = useCallback(() => {
    arena.runs().then((r) => { setRuns(r); setError(null); }).catch(setError);
  }, []);
  useEffect(() => { if (up) refresh(); }, [up, refresh]);
  const running = !!runs?.some((r) => r.branches.some((b) => b.status === 'running'));
  useEffect(() => {
    if (!up || !running) return;
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, [up, running, refresh]);
  return { runs, error, refresh };
}

/** The page's context for its panes (the main window and each popped-out one read their own). */
export function useArenaData(): ArenaCtx {
  const { health, error: healthError } = useArenaHealth();
  const { runs, error: runsError, refresh } = useRuns(!!health);
  return useMemo(() => ({ health, healthError, runs, runsError, refreshRuns: refresh }), [health, healthError, runs, runsError, refresh]);
}

export const useArena = () => usePaneContext<ArenaCtx>();
