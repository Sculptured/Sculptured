import { splitTeamMessage } from './teamMessage';
import { WORDS } from './vocab';
import type { EventRecord } from './generated/EventRecord';
import type { MessageIntent } from './generated/MessageIntent';
import type { MessageState } from './generated/MessageState';
import type { TurnStatus } from './generated/TurnStatus';
import { LIFECYCLE } from './status';

/** Where a queued message came from: the operator, the team's Job at start, or a handoff. */
export type MessageOrigin = 'operator' | 'job' | 'handoff' | 'agent' | 'system';

/** A handoff message, split into its parts. Every part is the message's own text, verbatim. */
export type Handoff = {
  /** `Handed over by …` (first handoff of a start) or `More work from …`. */
  kind: 'handed-over' | 'more-work';
  /** The first line, verbatim. */
  title: string;
  from: string[];
  /** Text between the first line and the first result heading: the agent's Job, if any. */
  job: string;
  results: { name: string; text: string }[];
};

export type OperatorItem = {
  kind: 'operator';
  key: string;
  seq: number;
  atMs: number;
  messageId: string;
  text: string;
  intent: MessageIntent;
  state: MessageState;
  reason?: string;
  provenance: string;
  /** The team's Job, sent by the system as the agent's first message when the team started. */
  atStart: boolean;
  origin: MessageOrigin;
  sender?: string;
  /** Set for a handoff whose text follows the handoff format. */
  handoff?: Handoff;
  /** The turn that read this message, once consumed. */
  turnId?: string;
};
export type ToolItem = {
  kind: 'tool';
  key: string;
  seq: number;
  callId: string;
  name: string;
  input: unknown;
  output?: string;
  isError?: boolean;
  parentCallId: string | null;
  provenance: string;
  calledMs: number;
  /** Set once a result arrives. */
  resultMs?: number;
  turnId: string | null;
};
export type AgentItem = { kind: 'agent'; key: string; seq: number; atMs: number; text: string; provenance: string; turnId: string | null };
export type TurnEndItem = {
  kind: 'turn-end';
  key: string;
  seq: number;
  turnId: string;
  status: TurnStatus;
  provenance: string;
  /** From the turn's start event to its end event; null when the start was not seen. */
  durationMs: number | null;
  /** Summed usage reports for the turn; null when none arrived. */
  tokens: { input: number; output: number } | null;
};
export type NoticeItem = {
  kind: 'notice';
  key: string;
  seq: number;
  text: string;
  provenance?: string;
  /** A verbatim runtime-originated part of `text`, shown as raw evidence. */
  raw?: string;
  /** Something went wrong: shown in red rather than as a quiet divider. */
  alarm?: boolean;
  /** A session (tenure) began or ended here; `cause` is why it began (`TenureCause`). */
  session?: { phase: 'started' | 'ended'; cause?: string };
};
export type Item =
  | OperatorItem
  | ToolItem
  | AgentItem
  | TurnEndItem
  | { kind: 'unrecognized'; key: string; seq: number; raw: unknown }
  | NoticeItem;

const RANK: Record<MessageState, number> = { queued: 0, delivered: 1, consumed: 2, failed: 3 };

/** Same reason the store records when a tenure ends with messages it never read. */
export const STOPPED_BEFORE_READING = 'the agent stopped before reading this message';

/** The system actor that sends a team's Job at start. Must match Rust `reconciler::TEAM_START_ACTOR`. */
export const TEAM_START_ACTOR = 'team-start';

/** The system actor that hands work downstream. Must match Rust `HANDOFF_ACTOR`. */
export const HANDOFF_ACTOR = 'handoff';

const RESULT_HEADING = /^--- (.+?)['’]s result ---\s*$/;

/**
 * Splits a handoff message: a first line `Handed over by A, B` or `More work from A`, an
 * optional Job, then each upstream's result under `--- A's result ---`. Null when the first
 * line is not in that form; the caller then shows the text as it is.
 */
export function parseHandoff(text: string): Handoff | null {
  const lines = splitTeamMessage(text).text.split('\n');
  const title = lines[0].trim();
  const m = /^(Handed over by|More work from)\s+(.+?)\s*$/.exec(title);
  if (!m) return null;
  const from = m[2]
    .split(/\s*,\s*/)
    .map((s) => s.trim())
    .filter(Boolean);
  const job: string[] = [];
  const results: { name: string; text: string[] }[] = [];
  for (const line of lines.slice(1)) {
    const h = RESULT_HEADING.exec(line);
    if (h) results.push({ name: h[1], text: [] });
    else if (results.length) results[results.length - 1].text.push(line);
    else job.push(line);
  }
  return {
    kind: m[1] === 'Handed over by' ? 'handed-over' : 'more-work',
    title,
    from,
    job: job.join('\n').trim(),
    results: results.map((r) => ({ name: r.name, text: r.text.join('\n').trim() })),
  };
}

export function originOf(e: Pick<EventRecord, 'actor'>): MessageOrigin {
  if (e.actor.kind === 'system' && e.actor.id === TEAM_START_ACTOR) return 'job';
  if (e.actor.kind === 'system' && e.actor.id === HANDOFF_ACTOR) return 'handoff';
  return e.actor.kind === 'seat' ? 'agent' : e.actor.kind === 'human' ? 'operator' : 'system';
}

/** Folds a seat's events into conversation items. Message state only moves forward. */
export function conversationItems(events: EventRecord[]): Item[] {
  const items: Item[] = [];
  const messages = new Map<string, OperatorItem>();
  const tools = new Map<string, ToolItem>();
  const turnStart = new Map<string, number>();
  const usage = new Map<string, { input: number; output: number }>();
  const advance = (id: string, state: MessageState, reason?: string) => {
    const m = messages.get(id);
    if (!m || m.state === 'failed') return;
    if (RANK[state] > RANK[m.state]) {
      m.state = state;
      // A reason belongs to the state it came with (a held message's wait ends when it is delivered).
      m.reason = reason;
    }
  };
  let prov = '';
  const notice = (seq: number, text: string, raw?: string, alarm?: boolean, session?: NoticeItem['session']) =>
    items.push({ kind: 'notice', key: `n-${seq}`, seq, text, raw, provenance: prov, alarm, ...(session ? { session } : {}) });

  for (const e of events) {
    const b = e.body;
    prov = e.provenance;
    switch (b.type) {
      case 'message-queued': {
        const origin = originOf(e);
        const m: OperatorItem = {
          kind: 'operator',
          key: `m-${b.message_id}`,
          seq: e.seq,
          atMs: e.at_ms,
          messageId: b.message_id,
          text: b.text,
          intent: b.intent,
          state: 'queued',
          provenance: e.provenance,
          atStart: origin === 'job',
          origin,
          sender: e.actor.kind === "seat" ? e.actor.id : undefined,
          handoff: origin === 'handoff' ? (parseHandoff(b.text) ?? undefined) : undefined,
        };
        messages.set(b.message_id, m);
        items.push(m);
        break;
      }
      case 'message-delivered':
        advance(b.message_id, 'delivered');
        break;
      case 'message-consumed': {
        advance(b.message_id, 'consumed');
        const m = messages.get(b.message_id);
        if (m) m.turnId = b.turn_id;
        break;
      }
      case 'message-failed':
        advance(b.message_id, 'failed', b.reason);
        break;
      case 'message-held': {
        const m = messages.get(b.message_id);
        if (m?.state === 'queued') m.reason = b.reason;
        break;
      }
      case 'turn-started':
        turnStart.set(b.turn_id, e.at_ms);
        break;
      case 'usage-reported': {
        if (b.turn_id === null) break;
        const u = usage.get(b.turn_id) ?? { input: 0, output: 0 };
        u.input += b.input_tokens + b.cache_read_tokens + b.cache_write_tokens;
        u.output += b.output_tokens;
        usage.set(b.turn_id, u);
        break;
      }
      case 'agent-text':
        items.push({ kind: 'agent', key: `a-${e.seq}`, seq: e.seq, atMs: e.at_ms, text: b.text, provenance: e.provenance, turnId: b.turn_id });
        break;
      case 'tool-called': {
        const t: ToolItem = {
          kind: 'tool',
          key: `t-${b.call_id}-${e.seq}`,
          seq: e.seq,
          callId: b.call_id,
          name: b.name,
          input: b.input,
          parentCallId: b.parent_call_id,
          provenance: e.provenance,
          calledMs: e.at_ms,
          turnId: b.turn_id,
        };
        tools.set(b.call_id, t);
        items.push(t);
        break;
      }
      case 'tool-resulted': {
        const t = tools.get(b.call_id);
        if (t) {
          t.output = b.output;
          t.isError = b.is_error;
          t.resultMs = e.at_ms;
        } else {
          notice(e.seq, `result for an unseen tool call ${b.call_id}`);
        }
        break;
      }
      case 'turn-ended': {
        const start = turnStart.get(b.turn_id);
        items.push({
          kind: 'turn-end',
          key: `e-${e.seq}`,
          seq: e.seq,
          turnId: b.turn_id,
          status: b.status,
          provenance: e.provenance,
          durationMs: start === undefined ? null : Math.max(0, e.at_ms - start),
          tokens: usage.get(b.turn_id) ?? null,
        });
        break;
      }
      case 'unrecognized':
        items.push({ kind: 'unrecognized', key: `u-${e.seq}`, seq: e.seq, raw: b.raw });
        break;
      case 'seat-lifecycle-changed':
        notice(
          e.seq,
          `${LIFECYCLE[b.lifecycle].label}${b.reason ? ` — ${b.reason}` : ''}`,
          b.reason || undefined,
          b.lifecycle === 'failed' || b.lifecycle === 'attention',
        );
        break;
      case 'tenure-started':
        notice(e.seq, WORDS.sessionStarted(b.tenure, b.cause, b.driver), b.driver, undefined, { phase: 'started', cause: b.cause });
        break;
      case 'tenure-ended':
        // The process is gone: a message it had not read yet never will be.
        for (const m of messages.values()) {
          if (m.state === 'queued' || m.state === 'delivered') advance(m.messageId, 'failed', STOPPED_BEFORE_READING);
        }
        notice(e.seq, WORDS.sessionEnded(b.tenure, b.exit_code, b.signal), undefined, b.exit_code !== 0 && b.exit_code !== null, { phase: 'ended' });
        break;
      case 'events-lost':
        notice(e.seq, `${b.count} events lost — history has a gap`, undefined, true);
        break;
      case 'protection-applied':
        // The refused call's own result says it too; this marks it as Sculptured's doing.
        notice(e.seq, `Sculptured refused this call: ${b.reason.replace(/^Sculptured protection:\s*/, '')}`, b.reason);
        break;
      case 'context-ceiling-reached':
        notice(e.seq, `Context ${Math.round(b.used_tokens / 1000)}k passed the ${Math.round(b.ceiling / 1000)}k limit: this agent writes its notes and continues in a fresh conversation`);
        break;
      default:
        break;
    }
  }
  return items;
}

/** The turn still in progress at the end of these events, if any. A session end closes it. */
export function openTurn(events: EventRecord[]): string | null {
  let open: string | null = null;
  for (const e of events) {
    const b = e.body;
    if (b.type === 'turn-started') open = b.turn_id;
    else if (b.type === 'turn-ended' && b.turn_id === open) open = null;
    else if (b.type === 'tenure-ended') open = null;
  }
  return open;
}
