import { perMinute } from '../rate';
import type { PlantAction, SystemUnit, UsagePoint } from './model';

/** The trends: actions per minute per system, context per call, tokens and cost, and latency. Pure. */

export type SystemRate = { system: SystemUnit; perMin: (number | null)[]; reads: number; writes: number; abnormal: number };

/** Each system's actions per minute up to `end` (unknown before `since`), with its read, write and abnormal counts. */
export function ratesBySystem(systems: SystemUnit[], actions: PlantAction[], end: number, since?: number): SystemRate[] {
  return systems.map((system) => {
    const mine = actions.filter((a) => a.system === system.id);
    return {
      system,
      perMin: perMinute(mine.map((a) => a.at), end, since),
      reads: mine.filter((a) => a.effect === 'read').length,
      writes: mine.filter((a) => a.effect === 'write').length,
      abnormal: mine.filter((a) => a.outcome === 'refused' || a.outcome === 'error').length,
    };
  });
}

/** The last `n` context sizes reported (one per model call or report), oldest first, and the largest of all. */
export function contextSeries(usage: UsagePoint[], n = 30): { values: number[]; peak?: number } {
  const all = usage.flatMap((u) => (u.context !== undefined ? [u.context] : []));
  return { values: all.slice(-n), ...(all.length ? { peak: Math.max(...all) } : {}) };
}

/**
 * Tokens and cost summed from the reports. `priced` says whether every report that had tokens also had
 * a cost; when not, the cost shown is a floor.
 */
export function totals(usage: UsagePoint[]): { tokens?: number; costUsd?: number; priced: boolean } {
  const withTokens = usage.filter((u) => u.tokens !== undefined);
  const withCost = usage.filter((u) => u.costUsd !== undefined);
  return {
    ...(withTokens.length ? { tokens: withTokens.reduce((n, u) => n + u.tokens!, 0) } : {}),
    ...(withCost.length ? { costUsd: withCost.reduce((n, u) => n + u.costUsd!, 0) } : {}),
    priced: withTokens.every((u) => u.costUsd !== undefined),
  };
}

const quantile = (sorted: number[], q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]!;

/** Latency of the actions whose duration was recorded: median, 90th percentile and the latest ones. */
export function latency(actions: PlantAction[], n = 30): { count: number; median?: number; p90?: number; recent: number[] } {
  const ms = actions.flatMap((a) => (a.ms !== undefined ? [a.ms] : []));
  if (ms.length === 0) return { count: 0, recent: [] };
  const sorted = [...ms].sort((a, b) => a - b);
  return { count: ms.length, median: quantile(sorted, 0.5), p90: quantile(sorted, 0.9), recent: ms.slice(-n) };
}

/** A duration in words: "420 ms", "3.2 s", "2 min 5 s". */
export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`;
}
