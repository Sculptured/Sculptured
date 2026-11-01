// How a rerun restores the world, in plain words: the per-part plan lines, the approximate mark,
// the parts a comparison or check left out, what a step saved and the run's world.
import type { Event, PartPlan, RestoredPart, RunStartPayload, ToolCallPayload } from './types';

/** The mark on a branch some part of which was restored approximately. */
export const APPROXIMATE_LABEL = 'Approximate restore';
/** The extra line under a comparison with an approximately restored side. */
export const APPROXIMATE_NOTE = 'Differences may come from the approximate restore as well as the change.';
/** Under a plan that replays a part. */
export const REPLAY_NOTE = "A replayed part starts fresh with the run's seed and repeats its recorded steps.";

/** The part of runs recorded before integrations (`recorded.rs` `LEGACY_PART`). */
const LEGACY_PART = 'retail';

/** True when some part of the branch (or compare side) was restored approximately. */
export const isApproximate = (b: { restore_mode?: string | null; restored?: RestoredPart[] | null }) =>
  !!b.restored?.some((p) => p.how === 'approximate');

const about = (s: number) => (s < 90 ? `about ${Math.max(1, Math.round(s))} s` : `about ${Math.round(s / 60)} min`);

/** "retail: from its save", "plant: replayed from the start, about 40 s", "isaac: approximate". */
export function planLine(p: PartPlan): string {
  if (p.how === 'save') return `${p.part}: from its save`;
  if (p.how === 'approximate') return `${p.part}: approximate`;
  return `${p.part}: replayed from the start${p.estimate_seconds != null ? `, ${about(p.estimate_seconds)}` : ''}`;
}

/** "Not compared: plant (it keeps no saves)"; empty when every part was compared or checked. */
export function notComparedLine(parts: string[] | null | undefined, where: 'compare' | 'check'): string {
  if (!parts?.length) return '';
  const why = where === 'check' ? 'no expected result to compare with' : parts.length > 1 ? 'they keep no saves' : 'it keeps no saves';
  return `Not compared: ${parts.join(', ')} (${why})`;
}

/** The parts saved after a tool call: its `saves`, else a legacy `snapshot` (of the retail part). */
export function savedParts(p: Partial<Pick<ToolCallPayload, 'saves' | 'snapshot'>>): string[] {
  if (p.saves) return Object.keys(p.saves);
  return typeof p.snapshot === 'string' && p.snapshot ? [LEGACY_PART] : [];
}

/** "Saved after this step", naming the parts when there are several; empty when nothing was saved. */
export function savedLine(p: Partial<Pick<ToolCallPayload, 'saves' | 'snapshot'>>): string {
  const parts = savedParts(p);
  if (!parts.length) return '';
  return parts.length > 1 ? `Saved after this step: ${parts.join(', ')}` : 'Saved after this step';
}

/** A fingerprint's plain values, e.g. "server_version 16.4" (nested values are left out). */
function pinned(fingerprint: unknown): string {
  if (!fingerprint || typeof fingerprint !== 'object') return '';
  return Object.entries(fingerprint)
    .filter(([, v]) => ['string', 'number', 'boolean'].includes(typeof v))
    .map(([k, v]) => `${k} ${String(v)}`)
    .join(', ');
}

export interface WorldPartLine {
  part: string;
  integration: string;
  /** What the part's fingerprint pins at the run's start (its versions). */
  version: string;
}

/** The run's world from its `run_start`; empty for a run recorded before integrations. */
export function worldParts(events: Event[]): WorldPartLine[] {
  const start = events.find((e) => e.kind === 'run_start')?.payload as RunStartPayload | undefined;
  return (start?.world ?? []).map((w) => ({
    part: w.part,
    integration: w.integration,
    version: pinned(start?.fingerprints?.[w.part]),
  }));
}

/** "World: retail (postgres · server_version 16.4) · plant (counter)"; empty for a run recorded before integrations. */
export function worldLine(events: Event[]): string {
  const parts = worldParts(events).map((p) => `${p.part} (${[p.integration, p.version].filter(Boolean).join(' · ')})`);
  return parts.length ? `World: ${parts.join(' · ')}` : '';
}
