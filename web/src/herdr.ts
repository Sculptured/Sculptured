import type { EventRecord } from './generated/EventRecord';
import type { Provenance } from './generated/Provenance';
import type { Runtime } from './generated/Runtime';
import type { SeatView } from './generated/SeatView';
import { compactNumber, plural, tokensLabel } from './format';
import { LIFECYCLE } from './status';
import { RUNTIME_LABEL } from './vocab';

/**
 * herdr's five agent states, plus the lifecycle states that have no running process.
 * Every state has a glyph and a word; colour (the tone) is only ever an addition.
 */
export type StateKey =
  | 'needs-you'
  | 'stuck'
  | 'slot'
  | 'working'
  | 'done'
  | 'idle'
  | 'unknown'
  | 'launching'
  | 'stopping'
  | 'stopped'
  | 'failed'
  | 'planned';

/** `reported`: the agent says it finished (grey). Green is reserved for a check that passed (verified), never a state. */
export type Tone = 'red' | 'vendor' | 'reported' | 'plain' | 'violet' | 'muted';
/** `tip`: what the state means, for tooltips, where the short word could mislead. */
export type AgentState = { key: StateKey; glyph: string; word: string; tone: Tone; tip?: string };

/** "done (reported)" only says a turn ended since you last looked, never that the work is right. */
export const DONE_TIP = 'turn finished; not verified';

/** "stuck" says the agent's handoffs cannot go on by themselves; the reason says why. */
export const STUCK_TIP = 'its handoffs cannot go on by themselves';

/** "waiting for a slot": a message waits until fewer of the run's agents work at once. */
export const SLOT_TIP = 'its next message waits until fewer agents work at once (Run budget)';

/** Static frame of the working spinner, used when motion is reduced and in text. */
export const WORKING_GLYPH = '⣟';

const S: Record<StateKey, AgentState> = {
  'needs-you': { key: 'needs-you', glyph: '◉', word: 'needs you', tone: 'red' },
  stuck: { key: 'stuck', glyph: '▲', word: 'stuck', tone: 'red', tip: STUCK_TIP },
  slot: { key: 'slot', glyph: '⧗', word: 'waiting for a slot', tone: 'muted', tip: SLOT_TIP },
  working: { key: 'working', glyph: WORKING_GLYPH, word: 'working', tone: 'vendor' },
  done: { key: 'done', glyph: '✓', word: 'done (reported)', tone: 'reported', tip: DONE_TIP },
  idle: { key: 'idle', glyph: '○', word: 'idle', tone: 'plain' },
  unknown: { key: 'unknown', glyph: '∅', word: 'unknown', tone: 'violet' },
  launching: { key: 'launching', glyph: LIFECYCLE.launching.glyph, word: LIFECYCLE.launching.label, tone: 'muted' },
  stopping: { key: 'stopping', glyph: LIFECYCLE.stopping.glyph, word: LIFECYCLE.stopping.label, tone: 'muted' },
  stopped: { key: 'stopped', glyph: LIFECYCLE.stopped.glyph, word: LIFECYCLE.stopped.label, tone: 'muted' },
  failed: { key: 'failed', glyph: LIFECYCLE.failed.glyph, word: LIFECYCLE.failed.label, tone: 'red' },
  planned: { key: 'planned', glyph: LIFECYCLE.planned.glyph, word: LIFECYCLE.planned.label, tone: 'muted' },
};
export const STATES = S;

/**
 * The state shown for an agent. `done` means a turn ended since this browser last viewed
 * the agent; it only applies to a running, idle agent. A missing view is unknown.
 */
export function agentState(seat: (Pick<SeatView, 'lifecycle' | 'activity'> & Partial<Pick<SeatView, 'handoff_stuck' | 'slot_wait'>>) | undefined, done = false): AgentState {
  if (!seat) return S.unknown;
  switch (seat.lifecycle) {
    case 'attention':
      return S['needs-you'];
    case 'stale':
      return S.unknown;
    case 'running':
      switch (seat.activity) {
        case 'waiting-input':
          return S['needs-you'];
        case 'working':
          return S.working;
        case 'idle':
          // Its next message is held for a free slot: that is why it is idle, not a stuck handoff.
          if (seat.slot_wait) return S.slot;
          // Idle because its handoffs cannot go on by themselves: say so.
          if (seat.handoff_stuck) return S.stuck;
          return done ? S.done : S.idle;
        default:
          return seat.handoff_stuck ? S.stuck : S.unknown;
      }
    default:
      return S[seat.lifecycle];
  }
}

export function blockRank(key: StateKey): number {
  if (key === 'needs-you') return 0;
  if (key === 'stuck') return 1;
  if (key === 'failed') return 2;
  return 3;
}

/** Stable sort: blocked first, then the given (declaration) order. */
export function sortAgents<T>(items: T[], key: (t: T) => StateKey): T[] {
  return items
    .map((t, i) => ({ t, i, r: blockRank(key(t)) }))
    .sort((a, b) => a.r - b.r || a.i - b.i)
    .map((x) => x.t);
}

export type Vendor = { mark: string; name: string; cls: string };

export function vendor(runtime: Runtime | undefined): Vendor {
  switch (runtime) {
    case 'claude-code':
      return { mark: '✳', name: 'Claude', cls: 'v-claude' };
    case 'codex':
      return { mark: '◈', name: 'Codex', cls: 'v-codex' };
    case 'stub':
      return { mark: '◇', name: 'Practice', cls: 'v-practice' };
    case undefined:
      return { mark: '·', name: 'unknown', cls: 'v-other' };
    default:
      return { mark: '▪', name: RUNTIME_LABEL[runtime], cls: 'v-other' };
  }
}

/** Compact age for one-line rows: "now", "4m", "3h", "2d", or "unknown". */
export function shortAge(ms: number | null, now: number): string {
  if (ms === null) return 'unknown';
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return 'now';
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

export function tokensText(seat: Pick<SeatView, 'tokens_in' | 'tokens_out'> | undefined): string {
  if (!seat || seat.tokens_in === null || seat.tokens_out === null) return tokensLabel(null);
  return tokensLabel(seat.tokens_in + seat.tokens_out);
}

/** The whole run's tokens so far: what every agent reported, and how many have not reported. */
export function teamTokensText(seats: Pick<SeatView, 'tokens_in' | 'tokens_out'>[]): string {
  const known = seats.filter((s) => s.tokens_in !== null && s.tokens_out !== null);
  if (known.length === 0) return '';
  const total = known.reduce((n, s) => n + (s.tokens_in ?? 0) + (s.tokens_out ?? 0), 0);
  const missing = seats.length - known.length;
  if (missing === 0) return `${compactNumber(total)} tokens so far`;
  return `at least ${compactNumber(total)} tokens so far (${plural(missing, 'agent')} ${missing === 1 ? 'has' : 'have'} not reported)`;
}

/** Tool names as typed in a terminal: lower case. */
export function toolLabel(name: string): string {
  return name.toLowerCase();
}

export type ToolChip = {
  name: string;
  count: number;
  /** State of the latest call of this tool. */
  status: 'running' | 'ok' | 'error';
  /** Duration of the latest call in ms; null while it is still running. */
  ms: number | null;
  /** When the latest call started (event time, ms). */
  startedAt: number;
};

export type AgentActivity = {
  /** Tool chips for the latest turn, most recently used first, at most `max`. */
  chips: ToolChip[];
  /** The latest agent text line, verbatim. */
  lastText: string | null;
  /** Seq of the latest turn end; drives the "done" state. */
  lastTurnEndSeq: number | null;
  /** How many different tools the latest turn used (chips shows at most `max`). */
  toolCount?: number;
  /** The team's own limit refused a spawn in the latest turn: the refusal, verbatim. */
  limitNote?: string;
};

/** Tools of the latest turn, grouped by name. Pure: a function of the events only. */
export function activityOf(events: EventRecord[], max = 3): AgentActivity {
  const { start, lastText, lastTurnEndSeq } = latestTurn(events);
  const byName = toolChips(events, start);
  const chips = [...byName.values()].sort((a, b) => b.order - a.order).slice(0, max).map(({ order: _o, ...c }) => c);
  const limitNote = limitRefusal(events, start);
  return { chips, lastText, lastTurnEndSeq, toolCount: byName.size, ...(limitNote ? { limitNote } : {}) };
}

/** The latest spawn the team's own limit refused from `start` on (its reason names the setting). */
function limitRefusal(events: EventRecord[], start: number): string | undefined {
  for (let i = events.length - 1; i >= start; i--) {
    const b = events[i].body;
    if (b.type === 'report-rejected' && b.tool === 'spawn_agent' && b.reason.startsWith('Team limit you set')) return b.reason;
  }
  return undefined;
}

/** Where the latest turn starts, the latest agent text and the latest turn end. */
function latestTurn(events: EventRecord[]) {
  let start = 0;
  let lastText: string | null = null;
  let lastTurnEndSeq: number | null = null;
  for (let i = 0; i < events.length; i++) {
    const b = events[i].body;
    if (b.type === 'turn-started') start = i;
    else if (b.type === 'agent-text') lastText = b.text;
    else if (b.type === 'turn-ended') lastTurnEndSeq = events[i].seq;
  }
  return { start, lastText, lastTurnEndSeq };
}

/** One chip per tool name used from `start` on, with its latest call's state and order. */
function toolChips(events: EventRecord[], start: number): Map<string, ToolChip & { order: number }> {
  const calls = new Map<string, { name: string; at: number }>();
  const byName = new Map<string, ToolChip & { order: number }>();
  let order = 0;
  for (let i = start; i < events.length; i++) {
    const e = events[i];
    const b = e.body;
    if (b.type === 'tool-called') {
      const name = toolLabel(b.name);
      calls.set(b.call_id, { name, at: e.at_ms });
      const prev = byName.get(name);
      byName.set(name, { name, count: (prev?.count ?? 0) + 1, status: 'running', ms: null, startedAt: e.at_ms, order: order++ });
    } else if (b.type === 'tool-resulted') {
      const c = calls.get(b.call_id);
      if (!c) continue;
      const chip = byName.get(c.name);
      if (chip) {
        chip.status = b.is_error ? 'error' : 'ok';
        chip.ms = Math.max(0, e.at_ms - c.at);
      }
    }
  }
  return byName;
}

export function durationText(ms: number | null): string {
  if (ms === null) return '';
  if (ms < 1000) return `${(ms / 1000).toFixed(1)}s`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)}s`;
  return `${Math.floor(ms / 60_000)}m`;
}

/** `⚒ bash ×5 ✓` for finished tools, `⚒ edit 0.4s …` for a running one. */
export function chipText(c: ToolChip, now: number): string {
  if (c.status === 'running') return `⚒ ${c.name} ${durationText(Math.max(0, now - c.startedAt))} …`;
  return `⚒ ${c.name}${c.count > 1 ? ` ×${c.count}` : ''} ${c.status === 'ok' ? '✓' : '✗'}`;
}

/**
 * Evidence tiers, as in dagr: `◆ observed` (seen by Sculptured in the agent's protocol, on
 * disk or in a captured tool result), `◆ verified` (an independent check), `◇ reported` (the
 * agent's own words), `≈ heuristic` (inferred). Missing provenance is unknown. An agent's
 * claim (`claim`) is never shown as observed or verified: what it says stays reported.
 */
export type Tier = { glyph: string; word: 'verified' | 'observed' | 'reported' | 'heuristic' | 'unknown'; detail: string };

const OBSERVED = new Set(['protocol-observed', 'filesystem-observed', 'captured-tool', 'operator']);

export function tierOf(p: Provenance | string | null | undefined, claim = false): Tier {
  if (p === null || p === undefined) return { glyph: '∅', word: 'unknown', detail: 'unknown' };
  if (p === 'heuristic') return { glyph: '≈', word: 'heuristic', detail: 'heuristic' };
  if (claim) return { glyph: '◇', word: 'reported', detail: p };
  if (p === 'verified') return { glyph: '◆', word: 'verified', detail: 'verified' };
  if (OBSERVED.has(p)) return { glyph: '◆', word: 'observed', detail: p };
  return { glyph: '◇', word: 'reported', detail: p };
}

/** Rolled-up state counts for a team header or the status line. */
export type Counts = Record<StateKey, number>;
export function emptyCounts(): Counts {
  return { 'needs-you': 0, stuck: 0, slot: 0, working: 0, done: 0, idle: 0, unknown: 0, launching: 0, stopping: 0, stopped: 0, failed: 0, planned: 0 };
}
