/** How many minutes the tool-call rate covers. */
export const RATE_MINUTES = 15;

/**
 * Calls in each of the last RATE_MINUTES minutes up to `end`, oldest first, from when each call was
 * made. A minute that starts before `since` (events older than that are not loaded) is unknown (null),
 * never 0.
 */
export function perMinute(times: number[], end: number, since?: number): (number | null)[] {
  const out: (number | null)[] = new Array<number>(RATE_MINUTES).fill(0);
  for (const at of times) {
    const minute = Math.floor(Math.max(0, end - at) / 60_000);
    if (minute < RATE_MINUTES) out[RATE_MINUTES - 1 - minute]! += 1;
  }
  if (since === undefined) return out;
  return out.map((n, i) => (end - (RATE_MINUTES - i) * 60_000 < since ? null : n));
}
