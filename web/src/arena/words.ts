// Arena control's plain words: how a part restores, sizes on disk, and times.
import type { PartRestore } from '../tests/types';

export const CANT_RESTORE = "Can't be restored";

/** "Exact from a save", "Exact by replay", "Approximate", or "Can't be restored"; "Unknown" when it didn't say. */
export function restoreWords(r: PartRestore | undefined): string[] {
  if (!r) return ['Unknown'];
  const words: string[] = [];
  if (r.exact === 'save') words.push('Exact from a save');
  if (r.exact === 'replay') words.push('Exact by replay');
  if (r.approximate) words.push('Approximate');
  return words.length ? words : [CANT_RESTORE];
}

const UNITS = ['bytes', 'KB', 'MB', 'GB', 'TB'];

/** 0 bytes, 512 KB, 1.5 GB: binary units, one decimal under 10. */
export function sizeWords(n: number): string {
  let v = Math.max(0, n);
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) { v /= 1024; u++; }
  const shown = u === 0 || v >= 10 ? Math.round(v).toString() : (Math.round(v * 10) / 10).toString();
  return `${shown} ${UNITS[u]}`;
}

/** "just now", "12 min ago", "3 h ago", "2 d ago"; the time itself when it can't be read. */
export function agoWords(at: string | number, now: number): string {
  const t = typeof at === 'number' ? at : Date.parse(at);
  if (Number.isNaN(t)) return String(at);
  const min = Math.floor(Math.max(0, now - t) / 60_000);
  if (min < 1) return 'just now';
  if (min < 60) return `${min} min ago`;
  if (min < 48 * 60) return `${Math.floor(min / 60)} h ago`;
  return `${Math.floor(min / (24 * 60))} d ago`;
}

/** How long a branch ran, from its start to its end; null while running or when unreadable. */
export function branchSeconds(b: { created_at: string; ended_at: string | null }): number | null {
  if (!b.ended_at) return null;
  const s = (Date.parse(b.ended_at) - Date.parse(b.created_at)) / 1000;
  return Number.isNaN(s) || s < 0 ? null : s;
}

/** "41.0 s", "3 min 20 s", "1 h 5 min". */
export function durationWords(s: number): string {
  if (s < 60) return `${s.toFixed(1)} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${Math.round(s - m * 60)} s`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}
