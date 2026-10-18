import type { IconName } from '../shell/icons';
import type { TeamRow } from '../shell/model';
import { allAgentsFinished } from '../shell/runFinished';
import { hasProcess } from '../status';

export type StatusTone = 'needs-you' | 'waiting' | 'working' | 'done' | 'idle';
/** A project's status as a drawn shape plus words: never colour alone. */
export type ProjectStatus = { icon: IconName; text: string; tone: StatusTone };

/**
 * The Standard look's header status: an icon and words, coloured by the status tokens. Agents that
 * finished their turns are reported done (grey), never verified; unknown stays unknown.
 */
export function statusPill(s: { attention: number; connected: boolean; finished: boolean; starting: boolean; live: boolean; working: number; total: number }): ProjectStatus {
  if (s.attention > 0) return { icon: 'alert', text: `Needs you · ${s.attention} of ${s.total}`, tone: 'needs-you' };
  if (!s.connected) return { icon: 'refresh', text: 'Reconnecting', tone: 'waiting' };
  if (s.starting) return { icon: 'loader', text: 'Starting', tone: 'waiting' };
  if (s.finished) return { icon: 'check-circle', text: `Finished (reported) · ${s.total} of ${s.total} done`, tone: 'done' };
  if (s.live && s.working > 0) return { icon: 'activity', text: `Running · ${s.working} of ${s.total} working`, tone: 'working' };
  if (s.live) return { icon: 'circle', text: 'Running · none working', tone: 'idle' };
  return { icon: 'stop', text: 'Stopped', tone: 'idle' };
}

/** A draft that has never run: whether it can start yet. */
export function draftStatus(errors: number | null): ProjectStatus {
  if (errors === null) return { icon: 'pencil', text: 'Draft · not started', tone: 'idle' };
  if (errors > 0) return { icon: 'alert', text: `Draft · ${errors} to fix`, tone: 'waiting' };
  return { icon: 'pencil', text: 'Draft · ready to start', tone: 'idle' };
}

/** Any project's status from its row: a draft, connecting, or its live state. `errors`: known setup problems. */
export function teamStatus(row: TeamRow | undefined, errors: number | null = null): ProjectStatus {
  if (!row || row.draftOnly) return draftStatus(errors);
  if (!row.loaded) return { icon: 'loader', text: row.connected ? 'Loading' : 'Connecting', tone: 'waiting' };
  const views = row.agents.map((a) => a.seat).filter((s) => !!s);
  const live = views.some((s) => hasProcess(s.lifecycle) || s.lifecycle === 'attention');
  const starting = views.some((s) => s.lifecycle === 'launching');
  return statusPill({
    attention: row.counts['needs-you'],
    connected: row.connected,
    finished: allAgentsFinished(row.agents),
    starting,
    live,
    working: row.counts.working,
    total: row.agents.length,
  });
}
