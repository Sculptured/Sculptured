import type { IconName } from './icons';
import type { AgentRow } from './model';

/**
 * Every agent of a running team has finished its work for now: each is running, idle, has
 * finished at least one turn in this start, and nothing waits on it, on you or on another agent.
 * This says only that the turns ended (protocol-observed); it never says the work is right.
 */
type Seat = NonNullable<AgentRow['seat']>;

const idleAndQuiet = (s: Seat) => s.lifecycle === 'running' && s.activity === 'idle' && !s.current_turn && !s.current_tool;
const nothingOpen = (s: Seat) => !(s.open_questions > 0 || s.restore_offer || s.handoff_stuck || s.slot_wait);
const nothingWaited = (s: Seat, a: AgentRow) => !(s.waiting_for && s.waiting_for.length > 0) && !(a.waitingFor && a.waitingFor.length > 0);

function agentFinished(a: AgentRow): boolean {
  const s = a.seat;
  return !!s && idleAndQuiet(s) && nothingOpen(s) && nothingWaited(s, a) && a.activity?.lastTurnEndSeq != null;
}

export function allAgentsFinished(rows: AgentRow[]): boolean {
  return rows.length > 0 && rows.every(agentFinished);
}

/** Identifies one finished state: a later turn by any agent makes a new one (a dismissed summary comes back). */
export function finishedKey(rows: AgentRow[]): string {
  return rows.map((a) => `${a.id}:${a.activity?.lastTurnEndSeq ?? ''}`).join('|');
}

/** An agent's latest reply, first lines only, as it said it; null when none was captured. */
export function lastReply(a: AgentRow, max = 160): string | null {
  const text = a.activity?.lastText?.trim();
  if (!text) return null;
  const flat = text.split('\n').map((l) => l.trim()).filter(Boolean).join(' ');
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

/** The check line for the summary: only a check Sculptured ran and that passed counts as verified. */
export function checkLine(a: AgentRow): { word: string; tone: string; icon: IconName; verified: boolean } {
  const c = a.seat?.check;
  if (!c) return { word: 'no check set · not verified', tone: 'muted', icon: 'circle', verified: false };
  if (c.passed === null) return { word: `check running: ${c.command}`, tone: 'muted', icon: 'loader', verified: false };
  if (c.passed) return { word: `check passed: ${c.command}`, tone: 'tone-verified', icon: 'shield-check', verified: true };
  return { word: `check failed: ${c.command}${c.timed_out ? ' (timed out)' : c.exit_code !== null ? ` (exit ${c.exit_code})` : ''}`, tone: 'tone-red', icon: 'x', verified: false };
}

/** The folded summary in one line: what the agents reported and what was verified, never more. */
export function finishedLine(rows: AgentRow[]): string {
  const verified = rows.filter((a) => checkLine(a).verified).length;
  return `Run finished · ${rows.length} reported done · ${verified} verified`;
}
