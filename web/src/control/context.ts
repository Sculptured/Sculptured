import { usePaneContext } from '../workspace/registry';
import type { ControlSource, FeedItem } from './source';
import type { AlarmsApi } from './useAlarms';

/** What every Control pane reads (usePaneContext): the source, the master caution and the page's own actions. */
export type ControlCtx = {
  kind: ControlSource['kind'];
  team: string;
  source: ControlSource;
  alarms: AlarmsApi;
  /** Brings a pane type forward (focuses it, or adds it). A no-op in a popped-out window. */
  show(type: string): void;
  /** Ticks every 15 seconds. */
  now: number;
};

export const useControl = () => usePaneContext<ControlCtx>();

/** An agent's name by id, else its id; "the team" for an entry about no one agent. */
export function nameIn(source: Pick<ControlSource, 'agents'>, id: string | undefined): string {
  if (!id) return 'the team';
  return source.agents.find((a) => a.id === id)?.name ?? id;
}

/** A sentence without the agent's name at its start, for a row whose own column names the agent. */
export const afterName = (text: string, who: string) => (text.startsWith(`${who} `) ? text.slice(who.length + 1) : text);

/** The id of the alarm a feed entry raises (as alarms.ts makes it), if it raises one. */
export const alarmIdOf = (item: FeedItem) => (item.alarm ? `${item.alarm}:${item.alarmKey ?? item.id}` : undefined);

const two = (n: number) => String(n).padStart(2, '0');

/** A moment as a 24-hour wall clock, "14:31:05". */
export function clockAt(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

/** A span as "hh:mm:ss" (hours keep counting past 99). */
export function span(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${two(Math.floor(s / 3600))}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}`;
}
