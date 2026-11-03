import type { Alarm } from '../alarms';
import type { FeedItem } from '../source';

/** Pure parts of the Timeline: its time window, each lane's turns and marks, and the axis. */

export type Window = 'all' | '1h' | '15m';
export const WINDOWS: { id: Window; label: string; ms?: number }[] = [
  { id: 'all', label: 'Whole run' },
  { id: '1h', label: 'Last hour', ms: 3_600_000 },
  { id: '15m', label: 'Last 15 minutes', ms: 900_000 },
];

export type Range = { start: number; end: number };

/** From the first entry (or the window's start) to now; null when there is nothing to draw. */
export function rangeOf(feed: FeedItem[], window: Window, now: number): Range | null {
  if (feed.length === 0) return null;
  const end = Math.max(now, feed[feed.length - 1].at);
  const ms = WINDOWS.find((w) => w.id === window)?.ms;
  const start = ms ? end - ms : feed[0].at;
  return { start, end: end > start ? end : start + 1000 };
}

/** Where `at` falls in the range, 0 to 100. */
export const pct = (r: Range, at: number) => Math.min(100, Math.max(0, ((at - r.start) / (r.end - r.start)) * 100));
export const inRange = (r: Range, at: number) => at >= r.start && at <= r.end;

export type Bar = { from: number; to: number };

/** An agent's turns, from each turn's start to its end (an open turn runs to the end of the range). */
export function turnsOf(items: FeedItem[], end: number): Bar[] {
  const out: Bar[] = [];
  let open: number | null = null;
  for (const i of items) {
    if (i.kind === 'turn-started') {
      if (open !== null) out.push({ from: open, to: i.at });
      open = i.at;
    } else if (i.kind === 'turn-ended' && open !== null) {
      out.push({ from: open, to: i.at });
      open = null;
    }
  }
  if (open !== null) out.push({ from: open, to: end });
  return out;
}

export type Mark = { id: string; at: number; tone: 'tick' | 'alarm' | 'ask'; text: string };

/** One mark per entry that is not a turn's start or end: alarms red, questions and changes amber. */
export function marksOf(items: FeedItem[], alarmIds: Set<string>, alarmOf: (i: FeedItem) => string | undefined): Mark[] {
  return items
    .filter((i) => i.kind !== 'turn-started' && i.kind !== 'turn-ended')
    .map((i) => {
      const id = alarmOf(i);
      const tone = id && alarmIds.has(id) ? 'alarm' : /^(question-raised|change-proposed)$/.test(i.kind) ? 'ask' : 'tick';
      return { id: i.id, at: i.at, tone, text: i.text };
    });
}

export const alarmMarks = (alarms: Alarm[]): Mark[] => alarms.map((a) => ({ id: a.id, at: a.at, tone: 'alarm', text: a.text }));

/** Five evenly spaced axis labels across the range. */
export function axisOf(r: Range): number[] {
  return [0, 0.25, 0.5, 0.75, 1].map((f) => r.start + f * (r.end - r.start));
}

/** A keyboard step along the track: 1% of the range, 10% with Shift; Home and End go to the ends. */
export function stepTime(r: Range, at: number, key: string, shift: boolean): number | null {
  const step = (r.end - r.start) * (shift ? 0.1 : 0.01);
  const clamp = (t: number) => Math.min(r.end, Math.max(r.start, t));
  if (key === 'ArrowRight') return clamp(at + step);
  if (key === 'ArrowLeft') return clamp(at - step);
  if (key === 'Home') return r.start;
  if (key === 'End') return r.end;
  return null;
}
