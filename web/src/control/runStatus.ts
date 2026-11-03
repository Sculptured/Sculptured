import type { AgentLine, ControlSource, Tone } from './source';

/** The header's readings, worked out from a source: the run's status and its agents counted by status. */

export type Reading = { glyph: string; word: string; tone: Tone };

/**
 * The run as a glyph plus a word: the source's own word when it knows better (a simulated branch's
 * outcome), else not started, stopped, paused (every live agent), or running.
 */
export function runStatus(s: Pick<ControlSource, 'agents' | 'running' | 'runStatus'>): Reading {
  if (s.runStatus) return s.runStatus;
  if (s.agents.length > 0 && s.agents.every((a) => a.status.tone === 'unknown')) return { glyph: '∅', word: 'unknown (not connected)', tone: 'unknown' };
  if (!s.running) return s.agents.length === 0 ? { glyph: '○', word: 'not started', tone: 'idle' } : { glyph: '■', word: 'stopped', tone: 'idle' };
  const live = s.agents.filter((a) => a.status.tone === 'ok' || a.status.word === 'paused' || a.status.word === 'pausing');
  if (live.length > 0 && live.every((a) => a.status.word === 'paused')) return { glyph: '‖', word: 'paused', tone: 'idle' };
  return { glyph: '●', word: 'running', tone: 'ok' };
}

const ORDER: Tone[] = ['bad', 'warn', 'ok', 'idle', 'unknown'];

/** The agents by status word, problems first: "▲ 1 needs attention", "● 2 running", "■ 1 stopped". */
export function countsOf(agents: AgentLine[]): { glyph: string; word: string; tone: Tone; n: number }[] {
  const by = new Map<string, { glyph: string; word: string; tone: Tone; n: number }>();
  for (const a of agents) {
    const g = by.get(a.status.word);
    if (g) g.n += 1;
    else by.set(a.status.word, { ...a.status, n: 1 });
  }
  return [...by.values()].sort((x, y) => ORDER.indexOf(x.tone) - ORDER.indexOf(y.tone));
}
