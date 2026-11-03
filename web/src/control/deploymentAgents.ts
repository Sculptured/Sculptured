import { SKIP_WORDS } from '../errors';
import { plural } from '../format';
import type { SeatView } from '../generated/SeatView';
import type { TeamControlResult } from '../generated/TeamControlResult';
import { describeActivity } from '../progress/activity';
import { hasProcess, LIFECYCLE } from '../status';
import { WORDS } from '../vocab';
import { sessionKey } from './deploymentFeed';
import type { ActionReport, AgentLine, Availability, Tone } from './source';

/** A deployment's agents, and whether each control can run now, in plain words. */

/** Each agent's control mode from `GET /api/teams/:id/controls` (null: none); undefined while unknown. */
export type Modes = Record<string, string | null | undefined>;
type Status = AgentLine['status'];

export const NOTHING_RUNNING = 'Nothing is running';
export const CONNECTING = 'Connecting…';
const OK: Availability = { ok: true };
const no = (why: string): Availability => ({ ok: false, why });
const isPaused = (mode: string | null | undefined) => mode === 'paused' || mode === 'pausing';
/** The server's own test for what Stop everything now stops (hostd may still run it). */
const mayHaveProcess = (s: SeatView) => hasProcess(s.lifecycle) || s.lifecycle === 'attention';

const TONE: Record<SeatView['lifecycle'], Tone> = {
  planned: 'idle', launching: 'ok', running: 'ok', stopping: 'idle', stopped: 'idle', stale: 'warn', attention: 'warn', failed: 'bad',
};

/** Glyph, word and tone: a problem first, then a pause, then the lifecycle. Disconnected: last known. */
export function statusOf(seat: SeatView, mode: string | null | undefined, connected: boolean): Status {
  const problem = seat.lifecycle === 'failed' || seat.lifecycle === 'attention' || seat.lifecycle === 'stale';
  const base: Status = !problem && mode === 'paused'
    ? { glyph: '‖', word: 'paused', tone: 'idle' }
    : !problem && mode === 'pausing'
      ? { glyph: '⧗', word: 'pausing', tone: 'idle' }
      : { glyph: LIFECYCLE[seat.lifecycle].glyph, word: LIFECYCLE[seat.lifecycle].label, tone: TONE[seat.lifecycle] };
  return connected ? base : { ...base, word: `${base.word} (last known)`, tone: 'unknown' };
}

/** The alarm the agent's lifecycle raises now, keyed by its session (as its event is, in the feed). */
function agentAlarm(seat: SeatView): AgentLine['alarm'] {
  if (seat.lifecycle !== 'failed' && seat.lifecycle !== 'attention') return undefined;
  return { rule: seat.lifecycle, key: sessionKey(seat.id, seat.tenure) };
}

export function agentLine(seat: SeatView, name: string, mode: string | null | undefined, connected: boolean, now: number): AgentLine {
  const known = seat.tokens_in !== null && seat.tokens_out !== null;
  const activity = describeActivity(seat, now);
  const alarm = agentAlarm(seat);
  return {
    id: seat.id,
    name,
    status: statusOf(seat, mode, connected),
    ...(activity !== 'unknown' ? { activity } : {}),
    ...(known ? { tokens: { in: seat.tokens_in ?? 0, out: seat.tokens_out ?? 0 } } : {}),
    ...(alarm ? { alarm } : {}),
  };
}

export const ALREADY_STOPPING = 'Already stopping.';

/** Any other control mode (a soft or hard stop, a budget's) means it is already being stopped. */
export function pauseCan(seat: SeatView, mode: string | null | undefined): Availability {
  if (mode === 'paused') return no(SKIP_WORDS.already_paused);
  if (mode === 'pausing') return no('Already pausing.');
  if (mode || seat.lifecycle === 'stopping') return no(ALREADY_STOPPING);
  return seat.lifecycle === 'running' ? OK : no(SKIP_WORDS.not_running);
}

export function resumeCan(mode: string | null | undefined): Availability {
  if (mode === undefined) return no('Checking whether it is paused…');
  return isPaused(mode) ? OK : no(SKIP_WORDS.not_paused);
}

export function stopCan(seat: SeatView): Availability {
  if (mayHaveProcess(seat) && seat.lifecycle !== 'stopping') return OK;
  return no(seat.lifecycle === 'stopping' ? WORDS.agentAlreadyStopping : WORDS.noProcessToStop(LIFECYCLE[seat.lifecycle].label));
}

export function messageCan(seat: SeatView, text: string): Availability {
  if (seat.lifecycle !== 'running') return no(WORDS.sendNeedsRunning(LIFECYCLE[seat.lifecycle].label));
  return text.trim() ? OK : no('Type a message to send.');
}

export const ALREADY_RUNNING = 'The team is already running.';
type TeamCan = Record<'start' | 'pauseAll' | 'resumeAll' | 'emergencyStop', Availability>;

/** Start, Pause all, Resume all and Emergency stop, from the agents and their control modes. */
export function teamCan(seats: SeatView[], modes: Modes, loading: boolean): TeamCan {
  if (loading) return { start: no(CONNECTING), pauseAll: no(CONNECTING), resumeAll: no(CONNECTING), emergencyStop: no(CONNECTING) };
  const live = seats.filter(mayHaveProcess);
  const paused = seats.filter((s) => isPaused(modes[s.id]));
  return {
    start: live.length > 0 ? no(ALREADY_RUNNING) : OK,
    pauseAll: pauseAllCan(seats, modes, paused.length > 0),
    resumeAll: paused.length > 0 ? { ok: true, note: `Resumes ${plural(paused.length, 'agent')}` } : no('Nothing is paused'),
    emergencyStop: live.length > 0 ? { ok: true, note: `Stops ${plural(live.length, 'agent')}` } : no(NOTHING_RUNNING),
  };
}

/** Pause all acts on running agents without a control mode; the note counts only those. */
function pauseAllCan(seats: SeatView[], modes: Modes, anyPaused: boolean): Availability {
  const running = seats.filter((s) => s.lifecycle === 'running');
  const free = running.filter((s) => !modes[s.id]);
  if (free.length > 0) return { ok: true, note: `Pauses ${plural(free.length, 'agent')}` };
  if (anyPaused) return no(SKIP_WORDS.already_paused);
  return no(running.length > 0 ? ALREADY_STOPPING : NOTHING_RUNNING);
}

/** A pause or resume's answer. `notes` (plain words about agents it acted on) is absent from older servers. */
type ControlResult = Pick<TeamControlResult, 'affected' | 'skipped'> & { notes?: { seat: string; reason: string }[] };

/** What a pause or resume did, with each agent it left alone and why (errors.ts words for each code). */
export function controlReport(verb: 'Paused' | 'Resumed', r: ControlResult, nameOf: (id: string) => string): ActionReport {
  const skipped = r.skipped.map((s) => ({ agent: s.seat, name: nameOf(s.seat), reason: SKIP_WORDS[s.code] ?? s.reason, ...(SKIP_WORDS[s.code] ? {} : { raw: true as const }) }));
  const notes = (r.notes ?? []).map((n) => ({ agent: n.seat, name: nameOf(n.seat), text: n.reason }));
  const n = r.affected.length;
  const head = n > 0 ? `${verb} ${plural(n, 'agent')}` : `No agent was ${verb.toLowerCase()}`;
  const text = skipped.length > 0 ? `${head}; ${plural(skipped.length, 'agent')} left as they were.` : `${head}.`;
  return notes.length > 0 ? { text, skipped, notes } : { text, skipped };
}

/** A per-agent pause or resume answers `{mode}` plus a TeamControlResult (the agents it covered). */
export function agentReport(verb: 'Paused' | 'Resumed', answer: unknown, nameOf: (id: string) => string): ActionReport | void {
  const r = answer as Partial<ControlResult> | null;
  if (r && Array.isArray(r.affected) && Array.isArray(r.skipped)) return controlReport(verb, { affected: r.affected, skipped: r.skipped, notes: r.notes }, nameOf);
}
