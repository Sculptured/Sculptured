import { useMemo } from 'react';
import { apiRequest, ApiError } from '../../api';
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

/** A test suite as Arena lists it. Read loosely: suites are new in Arena and their shape may grow. */
export type SuiteRow = { id: string; name?: string; team?: string; status?: string; tasks?: number; repeats?: number; pass_rate?: number; safety_violations?: number };

/** Suites: the list, or why there is none to show (`missing`: this Arena has no suites yet). */
export type Suites = { rows: SuiteRow[] } | { missing: true } | { error: string } | null;

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

/** Arena's suites, or `missing` when this Arena has no suites route (404 or not found). */
export async function readSuites(): Promise<Suites> {
  try {
    const v = await apiRequest<unknown>('GET', '/api/arena/suites');
    const list = Array.isArray(v) ? v : Array.isArray((v as { suites?: unknown })?.suites) ? (v as { suites: unknown[] }).suites : [];
    return { rows: list.filter((s): s is SuiteRow => !!s && typeof (s as SuiteRow).id === 'string') };
  } catch (e) {
    if (e instanceof ApiError && (e.status === 404 || e.code === 'arena_not_found' || e.code === 'not_found')) return { missing: true };
    throw e;
  }
}

export function useOrgData(): OrgCtx {
  const o = usePolled(controlOverview, OVERVIEW_MS, []);
  const r = usePolled(arena.runs, ARENA_MS, []);
  const s = usePolled(readSuites, ARENA_MS * 3, []);
  return useMemo(() => ({
    overview: o.data, overviewError: o.error, runs: r.data, runsError: r.error, suites: s.data ?? (s.error ? { error: s.error } : null),
  }), [o.data, o.error, r.data, r.error, s.data, s.error]);
}

export const useOrg = () => usePaneContext<OrgCtx>();
