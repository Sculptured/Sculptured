// Arena control's plain words: how a part restores, its clock, and sizes on disk.
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

export function clockWords(uses: boolean | undefined): string {
  if (uses == null) return 'Clock unknown';
  return uses ? 'Uses the arena clock' : 'Keeps its own time';
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
