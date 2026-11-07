import { useCallback, useMemo } from 'react';
import { useLocation, useSearchParams } from 'react-router-dom';
import { loadComparison } from '../tests/ComparePage';
import { arena } from '../tests/api';
import { branchNames, inOrder, layout, summarize, type Step } from '../tests/model';
import type { BranchDetail, Change, RestoreMode } from '../tests/types';
import { agentsOf, approvalsOf, DECIDED_BY_AGENTS, feedOf, outcomeOf, toolCallsPerMin } from './simulatedFeed';
import { arenaPlant } from './plant/arenaPlant';
import { useArenaRun, type ArenaRunData } from './simulatedData';
import { rowsAt, saveAt } from './simulatedWorld';
import { ActionError, type Action, type ActionReport, type ControlSource, type SimulatedTimeline, type TeamActions } from './source';

/** Why the team's controls do nothing on a simulated run, in visible words. */
export const ARENA_CANT_CONTROL = "Arena runs can't be paused or stopped from here yet";
export const START_IN_ARENA = 'Start runs from Arena control';
export const NO_MESSAGES = "Agents in an Arena run can't be sent messages; fork with changed instructions instead";

/** An action that cannot run: disabled with `why`, and pressing it anyway reports the refusal. */
const unavailable = (why: string): Action => ({
  can: { ok: false, why },
  run: async (): Promise<ActionReport> => ({ text: why, skipped: [], refused: true }),
});

/** The run's status before its branch is read: unknown (Arena not answering, or the read failed), or loading. */
const UNKNOWN = { glyph: '∅', word: 'unknown', tone: 'unknown' as const };
const LOADING = { glyph: '◌', word: 'loading', tone: 'unknown' as const };

const TEAM: TeamActions = {
  start: unavailable(START_IN_ARENA),
  stop: unavailable(ARENA_CANT_CONTROL),
  pauseAll: unavailable(ARENA_CANT_CONTROL),
  resumeAll: unavailable(ARENA_CANT_CONTROL),
  emergencyStop: unavailable(ARENA_CANT_CONTROL),
};

/** The cursor: the `?step=` the operator moved to, else the latest step (so a running branch's cursor follows it). */
function useCursor(steps: Step[]) {
  const [sp, setSp] = useSearchParams();
  const { state } = useLocation();
  const asked = Number(sp.get('step') ?? NaN);
  const cursor = steps.some((s) => s.seq === asked) ? asked : steps.at(-1)?.seq ?? 0;
  const setCursor = useCallback((seq: number) => {
    setSp((p) => { const n = new URLSearchParams(p); n.set('step', String(seq)); return n; }, { replace: true, state });
  }, [setSp, state]);
  return { cursor, setCursor };
}

/** Where a fork started, in plain words (arenad may move it to the nearest step it can restore). */
function forkNotice(r: { start_seq: number; moved_from: number | null; reason: string | null }): string {
  if (r.moved_from == null) return `Forked from step ${r.start_seq}.`;
  return `Started from step ${r.start_seq} instead of ${r.moved_from}${r.reason ? `: ${r.reason}` : '.'}`;
}

function timelineOf(run: string, b: BranchDetail, cursor: ReturnType<typeof useCursor>, lay: ReturnType<typeof layout>): SimulatedTimeline {
  const steps = inOrder(lay);
  return {
    steps,
    saves: steps.filter((s) => s.snapshot).map((s) => s.seq),
    ...cursor,
    layout: lay,
    plan: (seq, restore: RestoreMode = 'exact') => arena.restorePlan(b.id, seq, restore),
    fork: async (seq, changes: Record<string, Change>, restore) => {
      const r = await arena.fork(b.id, seq, changes, restore);
      return { run, branch: r.branch_id, notice: forkNotice(r) };
    },
  };
}

/** What the branch's events say: agents, feed, approvals and the totals. Recomputed only when the branch changes. */
function useMapped(detail: BranchDetail | null) {
  return useMemo(() => {
    if (!detail) return { agents: [], feed: [], approvals: [], lay: null };
    return { agents: agentsOf(detail), feed: feedOf(detail), approvals: approvalsOf(detail), lay: layout(detail.events, detail.team, detail.start_seq) };
  }, [detail]);
}

function metricsOf(b: BranchDetail | null): ControlSource['metrics'] {
  if (!b) return { toolCallsPerMin: [] };
  const now = Date.now();
  const sm = summarize(b, b.events, now);
  const end = b.ended_at ? Date.parse(b.ended_at) : now;
  return { ...(sm.tokens ? { tokens: sm.tokens } : {}), ...(sm.cost_usd != null ? { costUsd: sm.cost_usd } : {}), toolCallsPerMin: toolCallsPerMin(b, end) };
}

function arenaOf(run: string, branchId: string, d: ArenaRunData): NonNullable<ControlSource['arena']> {
  const branches = d.branches.length ? d.branches : d.detail ? [d.detail] : [];
  return {
    run, branchId, branches, names: branchNames(branches), parts: d.parts, openAvailable: !!d.health?.open_available, reconnects: d.reconnects,
    ...(d.reason ? { reason: d.reason } : {}),
    ...(d.detail ? { branch: d.detail } : {}),
    // Why the branch can't be read, or (once it is) that Arena stopped answering since.
    ...(d.error ? { error: d.error } : d.health === null ? { error: d.healthError } : {}),
    // The other branch first: the change column then reads as what this branch changed.
    compare: (other) => loadComparison(other, branchId),
  };
}

function worldOf(b: BranchDetail, save: number | null): NonNullable<ControlSource['world']> {
  return {
    save,
    rows: (key?: string) => (save === null ? Promise.resolve({}) : rowsAt(b, save, key)),
    read: async (seq, keys, part) => (await arena.rows(b.id, seq, keys, part)).rows,
  };
}

/** A branch's title: "Task 27 · Rerun 1". */
function titleOf(b: BranchDetail | null, names: Map<string, string>): string {
  const task = (b?.events.find((e) => e.kind === 'run_start')?.payload as { task_id?: string } | undefined)?.task_id;
  return b ? `Task ${task ?? '?'} · ${names.get(b.id) ?? 'Branch'}` : 'Arena run';
}

/**
 * An Arena run branch as a ControlSource, through the `/api/arena/*` proxy: the branch and its
 * stream, the rows at the cursor's save, restore plans, fork and compare. Arena runs can't be
 * paused, stopped or messaged, so those controls are unavailable and say why.
 */
export function useSimulatedSource(run: string, branchId: string): ControlSource {
  const d = useArenaRun(run, branchId);
  const b = d.detail;
  const m = useMapped(b);
  const plant = useMemo(() => (b ? arenaPlant(b) : undefined), [b]);
  const cursor = useCursor(m.lay ? inOrder(m.lay) : []);
  const info = arenaOf(run, branchId, d);
  const save = b && m.lay ? saveAt(inOrder(m.lay).filter((s) => s.snapshot).map((s) => s.seq), cursor.cursor) : null;
  return {
    kind: 'simulated',
    title: titleOf(b, info.names),
    ...branchStatus(b, info.error),
    agents: m.agents,
    feed: m.feed,
    approvals: m.approvals,
    decide: async () => { throw new ActionError(DECIDED_BY_AGENTS); },
    agentControl: () => unavailable(ARENA_CANT_CONTROL),
    message: () => unavailable(NO_MESSAGES),
    team: TEAM,
    metrics: metricsOf(b),
    ...(plant ? { plant } : {}),
    ...(b && m.lay ? { timeline: timelineOf(run, b, cursor, m.lay) } : {}),
    ...(b ? { world: worldOf(b, save) } : {}),
    arena: info,
  };
}

function branchStatus(b: BranchDetail | null, error: unknown) {
  return {
    running: b?.status === 'running',
    ...(b ? { startedAt: Date.parse(b.created_at) } : {}),
    runStatus: b ? outcomeOf(b.status) : error ? UNKNOWN : LOADING,
    ...(b ? {} : { unread: true as const }),
    ...(b?.ended_at ? { endedAt: Date.parse(b.ended_at) } : {}),
  };
}
