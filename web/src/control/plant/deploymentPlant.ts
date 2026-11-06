import type { ActionClass } from '../../generated/ActionClass';
import type { EventRecord } from '../../generated/EventRecord';
import type { TeamDraft } from '../../generated/TeamDraft';
import type { Outcome, Permit, Plant, PlantAction, Rights, SystemUnit, UsagePoint } from './model';
import { placeTool, SYSTEMS } from './toolSystems';

/**
 * A deployment (a Sculptured team running for real) as a plant, from its events: each tool call
 * with its result (duration, outcome) and the interpreter's class, placed on a system by
 * `placeTool`; permits from permission questions and proposed changes; usage per report. Pure.
 */

type Body<T extends EventRecord['body']['type']> = Extract<EventRecord['body'], { type: T }>;
const seatOf = (e: EventRecord) => (e.subject.kind === 'seat' ? e.subject.id : e.actor.kind === 'seat' ? e.actor.id : undefined);
/** A seat's member id (`group.member@team` → `member`). */
export const memberOf = (seat: string) => seat.split('@')[0]!.split('.').pop()!;

const REFUSAL = /permission|denied|refus|not allowed|blocked by|safety rule|user rejected/i;

/** One tool call's result as an outcome; a refusal is an error the gate, a rule or the person gave. */
function outcomeOf(r: Body<'tool-resulted'> | undefined): { outcome: Outcome; reason?: string } {
  if (!r) return { outcome: 'running' };
  if (!r.is_error) return { outcome: 'ok' };
  const reason = r.output.slice(0, 500);
  return { outcome: REFUSAL.test(reason) ? 'refused' : 'error', reason };
}

type Calls = { results: Map<string, { body: Body<'tool-resulted'>; at: number }>; classes: Map<string, ActionClass> };

function indexCalls(events: EventRecord[]): Calls {
  const results = new Map<string, { body: Body<'tool-resulted'>; at: number }>();
  const classes = new Map<string, ActionClass>();
  for (const e of events) {
    if (e.body.type === 'tool-resulted') results.set(e.body.call_id, { body: e.body, at: e.at_ms });
    if (e.body.type === 'action-interpreted') classes.set(e.body.call_id, e.body.class);
  }
  return { results, classes };
}

function toolAction(e: EventRecord, b: Body<'tool-called'>, calls: Calls, draft: TeamDraft | undefined, seen: Map<string, SystemUnit>): PlantAction {
  const place = placeTool(b.name, calls.classes.get(b.call_id), draft?.components ?? []);
  if (!seen.has(place.system.id)) seen.set(place.system.id, place.system);
  const r = calls.results.get(b.call_id);
  return {
    id: `${e.seq}`, at: e.at_ms, ...(r ? { ms: Math.max(0, r.at - e.at_ms) } : {}), agent: seatOf(e) ?? 'unknown', system: place.system.id, tool: place.tool,
    effect: place.effect, effectBasis: place.effectBasis, ...outcomeOf(r?.body), request: b.input, result: r?.body.output,
  };
}

/** Who decided a proposed change, in words. */
const DECIDER = { mandate: "the team's rules", person: 'a person', 'nobody-in-time': 'nobody in time (refused)' } as const;

/** Permits: permission questions (decided by you, or by a rule) and proposed changes (by the rules or a person). */
function permitsOf(events: EventRecord[]): Map<string, Permit> {
  const out = new Map<string, Permit>();
  const set = (id: string, p: Partial<Permit>) => { const cur = out.get(id); if (cur) out.set(id, { ...cur, ...p }); };
  for (const e of events) {
    const b = e.body;
    const agent = seatOf(e);
    if (b.type === 'question-raised' && b.source === 'native-permission') out.set(b.question_id, { id: b.question_id, at: e.at_ms, ...(agent ? { agent } : {}), title: `use ${b.prompt.tool ?? 'a tool'}`, state: 'waiting' });
    if (b.type === 'question-answered') set(b.question_id, b.answer.kind === 'allow' ? { state: 'granted', by: 'you' } : b.answer.kind === 'deny' || b.answer.kind === 'expired' ? { state: 'refused', by: b.answer.kind === 'expired' ? 'nobody in time' : 'you' } : {});
    if (b.type === 'question-decided') set(b.question_id, { state: /allow|approve|yes/i.test(b.decision) ? 'granted' : 'refused', by: 'a decision rule' });
    if (b.type === 'change-proposed') out.set(b.proposal_id, { id: b.proposal_id, at: e.at_ms, ...(agent ? { agent } : {}), title: b.summary, state: 'waiting', ...(b.system ? { system: b.system } : {}) });
    if (b.type === 'change-decided') set(b.proposal_id, { state: b.approved ? 'granted' : 'refused', by: DECIDER[b.by] });
  }
  return out;
}

/** The first call of the permitted tool by the same agent after its permission question carries the permit. */
function linkPermits(actions: PlantAction[], events: EventRecord[], permits: Map<string, Permit>) {
  for (const e of events) {
    if (e.body.type !== 'question-raised' || e.body.source !== 'native-permission' || !permits.has(e.body.question_id)) continue;
    const tool = e.body.prompt.tool;
    const a = actions.find((x) => x.at >= e.at_ms && x.agent === seatOf(e) && !x.permit && (x.tool === tool || `mcp__${x.system.slice(4)}__${x.tool}` === tool));
    if (a) a.permit = e.body.question_id;
  }
}

/** Each agent's decision rights and gates, from the design: what it decides, whether it asks first, the team's rules. */
export function rightsOf(draft: TeamDraft | undefined, seats: string[]): Rights {
  const out: Rights = {};
  const rules = draft?.mandate?.rules?.length ?? 0;
  for (const seat of seats) {
    const a = draft?.agents.find((x) => x.id === memberOf(seat));
    if (!a) { out[seat] = []; continue; }
    const domains = a.advanced.authority?.domains ?? [];
    out[seat] = [
      ...(domains.length ? [`decides: ${domains.join(', ')}`] : []),
      a.advanced.permission === 'restricted' ? 'asks before using tools' : 'uses its tools without asking',
      ...(rules ? [`changes it proposes are checked against ${rules} rule${rules === 1 ? '' : 's'}`] : []),
    ];
  }
  return out;
}

function usageOf(events: EventRecord[]): UsagePoint[] {
  return events.flatMap((e): UsagePoint[] => {
    const agent = seatOf(e);
    const who = agent ? { agent } : {};
    if (e.body.type === 'context-reported') return [{ at: e.at_ms, ...who, context: e.body.used_tokens }];
    if (e.body.type !== 'usage-reported' || e.body.in_turn) return [];
    const b = e.body;
    return [{ at: e.at_ms, ...who, tokens: b.input_tokens + b.output_tokens + b.cache_read_tokens + b.cache_write_tokens, ...(b.cost ? { costUsd: b.cost.usd } : {}) }];
  });
}

/** The deployment's events as a plant. `seats` are the team's agents now (for their rights). */
export function deploymentPlant(events: EventRecord[], draft: TeamDraft | undefined, seats: string[]): Plant {
  const calls = indexCalls(events);
  const seen = new Map<string, SystemUnit>();
  const actions = events.flatMap((e) => (e.body.type === 'tool-called' ? [toolAction(e, e.body, calls, draft, seen)] : []));
  const permits = permitsOf(events);
  linkPermits(actions, events, permits);
  if (permits.size > 0 && !seen.has('approvals')) seen.set('approvals', SYSTEMS.approvals);
  return { systems: [...seen.values()], actions, permits: [...permits.values()], rights: rightsOf(draft, seats), usage: usageOf(events) };
}
