const BOUNDARY = /[\s\-_/·:.(]/;

const atWordStart = (hay: string, i: number) => i === 0 || BOUNDARY.test(hay[i - 1]);

/**
 * How well `query` matches `text`: higher is better, null when it does not match. Every
 * whitespace-separated term must match. A term found whole scores highest (more at the start of
 * a word, more the earlier it is); otherwise its letters must appear in order ("adag" finds
 * "Add a group"), scoring less the further apart they are.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const hay = text.toLowerCase();
  let total = 0;
  for (const term of query.toLowerCase().split(/\s+/).filter(Boolean)) {
    const at = hay.indexOf(term);
    if (at >= 0) {
      total += 100 + term.length * 4 + (atWordStart(hay, at) ? 40 : 0) - Math.min(at, 30);
      continue;
    }
    let from = 0;
    let last = -1;
    let gaps = 0;
    let starts = 0;
    for (const ch of term) {
      const j = hay.indexOf(ch, from);
      if (j < 0) return null;
      if (last >= 0) gaps += j - last - 1;
      if (atWordStart(hay, j)) starts++;
      last = j;
      from = j + 1;
    }
    total += 20 + starts * 8 - Math.min(gaps, 60);
  }
  return total;
}
