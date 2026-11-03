import { apiRequest } from '../api';
import type {
  BranchDetail, Change, CompareResponse, Event, ForkResponse, GcResponse, Health, NewRunResponse, RestoreMode, RestorePlan,
  RowsResponse, RunSummary, StatusEvent, TaskSummary, Team, UsageResponse, WorldResponse,
} from './types';

/** arenad, through controld's pass-through (`crates/controld/src/arena.rs`). */
const BASE = '/api/arena';
const id = encodeURIComponent;

export const arena = {
  health: () => apiRequest<Health>('GET', `${BASE}/health`),
  tasks: () => apiRequest<TaskSummary[]>('GET', `${BASE}/tasks`),
  defaultTeam: () => apiRequest<Team>('GET', `${BASE}/team/default`),
  runs: () => apiRequest<RunSummary[]>('GET', `${BASE}/runs`),
  startRun: (task_id: string, team: Team) => apiRequest<NewRunResponse>('POST', `${BASE}/runs`, { task_id, team }),
  deleteRun: (run: string) => apiRequest<{ dropped: number }>('DELETE', `${BASE}/runs/${id(run)}`),
  branch: (b: string) => apiRequest<BranchDetail>('GET', `${BASE}/branches/${id(b)}`),
  /** The rows `keys` as saved just after step `seq`, in `part` (else the step's part). */
  rows: (b: string, seq: number, keys: string[], part?: string) =>
    apiRequest<RowsResponse>('GET', `${BASE}/branches/${id(b)}/rows?seq=${seq}&keys=${id(keys.join(','))}${part ? `&part=${id(part)}` : ''}`),
  fork: (b: string, at_seq: number, changes: Record<string, Change> = {}, restore: RestoreMode = 'exact') =>
    apiRequest<ForkResponse>('POST', `${BASE}/branches/${id(b)}/fork`, { at_seq, changes, restore }),
  /** How each part of a fork at `at_seq` would be restored; a 409 names the part that can't be. */
  restorePlan: (b: string, at_seq: number, restore: RestoreMode) =>
    apiRequest<RestorePlan>('GET', `${BASE}/branches/${id(b)}/restore-plan?at_seq=${at_seq}&restore=${restore}`),
  compare: (a: string, b: string) => apiRequest<CompareResponse>('GET', `${BASE}/compare?a=${id(a)}&b=${id(b)}`),
  /** The benchmark's parts, each with what its integration says about itself. */
  world: () => apiRequest<WorldResponse>('GET', `${BASE}/world`),
  /** Bytes each integration keeps, the total and the limit. */
  usage: () => apiRequest<UsageResponse>('GET', `${BASE}/usage`),
  /** Drops the saves no recorded run uses; a 409 while a run is going or a cleanup is running. */
  gc: () => apiRequest<GcResponse>('POST', `${BASE}/gc`),
};

/** Follows a running branch: each new step, then its status changes. Returns a function that stops. */
export function follow(b: string, onEvent: (e: Event) => void, onStatus: (s: StatusEvent) => void): () => void {
  const es = new EventSource(`${BASE}/branches/${id(b)}/stream`);
  const listen = <T,>(name: string, fn: (v: T) => void) =>
    es.addEventListener(name, (m) => {
      try {
        fn(JSON.parse((m as MessageEvent).data) as T);
      } catch {
        /* a malformed frame: skip it */
      }
    });
  listen<Event>('event', onEvent);
  listen<StatusEvent>('status', onStatus);
  return () => es.close();
}
