import { useMemo } from 'react';
import { apiRequest } from '../../api';
import type { ControlOverview } from '../../generated/ControlOverview';
import { arena } from '../../tests/api';
import type { RunSummary } from '../../tests/types';
import { usePolled } from '../../usePolling';
import { usePaneContext } from '../../workspace/registry';

/**
 * What the organisation overview (Control's display level 1) reads: every deployment's unit
 * (`GET /api/control/overview`), Arena's recent runs, and Arena's test suites when Arena has them.
 * Each is polled on its own; one failing leaves the others on screen.
 */

import { readSuites, type Suites } from './suites';
export { readSuites, type Suites } from './suites';

export type OrgCtx = {
  overview: ControlOverview | null;
  overviewError: string;
  runs: RunSummary[] | null;
  runsError: string;
  suites: Suites;
};

const OVERVIEW_MS = 5_000;
const ARENA_MS = 10_000;

export const controlOverview = () => apiRequest<ControlOverview>('GET', '/api/control/overview');

export function useOrgData(): OrgCtx {
  const o = usePolled(controlOverview, OVERVIEW_MS, []);
  const r = usePolled(arena.runs, ARENA_MS, []);
  const s = usePolled(readSuites, ARENA_MS * 3, []);
  return useMemo(() => ({
    overview: o.data, overviewError: o.error, runs: r.data, runsError: r.error, suites: s.data ?? (s.error ? { error: s.error } : null),
  }), [o.data, o.error, r.data, r.error, s.data, s.error]);
}

export const useOrg = () => usePaneContext<OrgCtx>();
