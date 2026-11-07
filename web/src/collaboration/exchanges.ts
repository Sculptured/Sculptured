import type { EventRecord } from '../generated/EventRecord';
import { splitTeamMessage } from '../teamMessage';

export type ExchangeKind = 'updates' | 'exchanges' | 'questions' | 'results';
export type Exchange = {
  key: string; event: EventRecord; agent: string; from: string[]; origin: string;
  kind: ExchangeKind; label: string; text: string; status?: string; item?: string;
  reported: boolean;
};

type Body = EventRecord['body'];
type Row = Omit<Exchange, 'key' | 'event' | 'agent' | 'from' | 'origin' | 'reported'> & Partial<Pick<Exchange, 'from' | 'origin' | 'reported'>>;
type Ctx = { handoffs: Map<string, string[]>; messages: Set<string>; status: Map<string, string> };

const DISPATCH = /(?:^|[._/])(?:dispatch_work|complete_work)$/;

function indexExchanges(sorted: EventRecord[]): Ctx {
  const ctx: Ctx = { handoffs: new Map(), messages: new Set(), status: new Map() };
  for (const e of sorted) noteEvent(ctx, e.body);
  return ctx;
}

function noteEvent({ handoffs, messages, status }: Ctx, b: Body) {
  if (b.type === 'handed-over') handoffs.set(b.message_id, b.from.map(f => f.seat));
  if (b.type === 'message-queued') messages.add(b.message_id);
  if (b.type === 'message-delivered') status.set(b.message_id, 'Delivered');
  if (b.type === 'message-consumed') status.set(b.message_id, 'Read');
  if (b.type === 'message-held' && !status.has(b.message_id)) status.set(b.message_id, 'Waiting for a slot');
  if (b.type === 'message-failed') status.set(b.message_id, `Delivery failed: ${b.reason}`);
}

function originOf(e: EventRecord) {
  if (e.actor.kind === 'human') return 'You';
  return e.actor.kind === 'system' && e.actor.id === 'team-start' ? 'Team job' : 'System';
}

function queuedRow(e: EventRecord, b: Extract<Body, { type: 'message-queued' }>, { handoffs, status }: Ctx): Row & Pick<Exchange, 'from'> {
  const from = handoffs.get(b.message_id) ?? (e.actor.kind === 'seat' ? [e.actor.id] : []);
  return { from, origin: originOf(e), kind: 'exchanges', label: from.length ? 'Handoff' : 'Message',
    text: splitTeamMessage(b.text).text, status: status.get(b.message_id) ?? 'Queued · delivery not yet observed', item: `m-${b.message_id}` };
}

function unseenHandoffRow(b: Extract<Body, { type: 'handed-over' }>, { handoffs, status }: Ctx): Row {
  return { from: handoffs.get(b.message_id) ?? [], kind: 'exchanges', label: 'Handoff',
    text: 'Message content is outside the loaded history. Open the receiving agent to inspect it.', status: status.get(b.message_id) ?? 'Delivery unknown' };
}

function dispatchRow(e: EventRecord, b: Extract<Body, { type: 'tool-called' }>): Row {
  return { kind: 'results', label: b.name.endsWith('dispatch_work') ? 'Dispatch requested' : 'Result submitted',
    text: JSON.stringify(b.input, null, 2), reported: true, item: `t-${b.call_id}-${e.seq}` };
}

function rowFor(e: EventRecord, ctx: Ctx): Row | null {
  const b = e.body;
  switch (b.type) {
    case 'agent-text':
      return b.text.trim() ? { kind: 'updates', label: 'Update', text: b.text, reported: true, item: `a-${e.seq}` } : null;
    case 'message-queued': return queuedRow(e, b, ctx);
    case 'handed-over': return ctx.messages.has(b.message_id) ? null : unseenHandoffRow(b, ctx);
    case 'question-raised': return { kind: 'questions', label: 'Question raised', text: b.prompt.text, reported: true };
    case 'findings-reported': return { kind: 'results', label: 'Reported findings', text: b.summary, reported: true };
    case 'tool-called': return DISPATCH.test(b.name) ? dispatchRow(e, b) : null;
    default: return null;
  }
}

/** Only captured communication. Never infer collaboration from a planned connection. */
export function teamExchanges(events: EventRecord[]): Exchange[] {
  const sorted = [...new Map(events.map(e => [e.seq, e])).values()].sort((a, b) => a.seq - b.seq);
  const ctx = indexExchanges(sorted);
  const rows: Exchange[] = [];
  for (const e of sorted) {
    if (e.subject.kind !== 'seat') continue;
    const row = rowFor(e, ctx);
    if (row) rows.push({ key: `${e.seq}`, event: e, agent: e.subject.id, from: [], origin: '', reported: false, ...row });
  }
  return rows.reverse();
}
