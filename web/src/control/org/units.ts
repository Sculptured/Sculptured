import type { OverviewTeam } from '../../generated/OverviewTeam';
import type { RunSummary } from '../../tests/types';
import type { Tone } from '../source';

/** The organisation overview's units, pure: a deployment's state and alarm count, Arena's recent runs. */

export type UnitState = { glyph: string; word: string; tone: Tone };

/**
 * A deployment's state as a glyph and a word: failed or needs attention when an agent does,
 * running while one runs, stopped when set to stop, not started when it never ran.
 */
export function unitState(t: OverviewTeam): UnitState {
  if (!t.started) return { glyph: '○', word: 'not started', tone: 'idle' };
  if (t.failed > 0) return { glyph: '✕', word: `${t.failed} failed`, tone: 'bad' };
  if (t.attention > 0) return { glyph: '◉', word: 'needs attention', tone: 'bad' };
  if (t.running > 0) return { glyph: '⣟', word: 'running', tone: 'ok' };
  return t.desired === 'running' ? { glyph: '◌', word: 'starting or idle', tone: 'idle' } : { glyph: '■', word: 'stopped', tone: 'idle' };
}

/** A unit's alarms: agents failed or needing attention, open questions, and calls that failed in the last 15 minutes. */
export function unitAlarms(t: OverviewTeam): { count: number; words: string[] } {
  const words = [
    t.failed ? `${t.failed} agent${t.failed === 1 ? '' : 's'} failed` : '',
    t.attention ? `${t.attention} need${t.attention === 1 ? 's' : ''} attention` : '',
    t.open_questions ? `${t.open_questions} question${t.open_questions === 1 ? '' : 's'} open` : '',
    t.failed_actions ? `${t.failed_actions} call${t.failed_actions === 1 ? '' : 's'} failed (15 min)` : '',
  ].filter(Boolean);
  return { count: t.failed + t.attention + t.open_questions + t.failed_actions, words };
}

/** Tool calls in the latest minute, and in all 15. */
export function unitRate(t: OverviewTeam): { now: number; total: number } {
  return { now: t.actions_per_min.at(-1) ?? 0, total: t.actions_per_min.reduce((n, x) => n + x, 0) };
}

/** Units with alarms first, then running ones, then the rest by name: the order an operator scans. */
export function byAttention(a: OverviewTeam, b: OverviewTeam): number {
  const rank = (t: OverviewTeam) => (unitAlarms(t).count > 0 ? 0 : t.running > 0 ? 1 : 2);
  return rank(a) - rank(b) || a.name.localeCompare(b.name);
}

export type RecentRun = { run: string; branch: string; task: string; team: string; status: RunSummary['branches'][number]['status']; created: string; external: boolean };

/** Each run's first branch, newest first, at most `n`. */
export function recentRuns(runs: RunSummary[], n = 12): RecentRun[] {
  return [...runs].sort((x, y) => y.created_at.localeCompare(x.created_at)).slice(0, n).flatMap((r) => {
    const first = r.branches.find((b) => b.parent_id === null) ?? r.branches[0];
    if (!first) return [];
    const runner = (first as { runner?: unknown }).runner;
    return [{ run: r.id, branch: first.id, task: r.task_id, team: first.team.name, status: first.status, created: r.created_at, external: runner === 'external' || first.team.agents.some((a) => (a.kind as string) === 'external') }];
  });
}
