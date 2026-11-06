import { REFUSAL_WORDS } from '../alarms';
import type { BranchDetail, Event, RunStartPayload } from '../../tests/types';
import type { Outcome, Permit, Plant, PlantAction, Rights, SystemUnit, UsagePoint } from './model';

/** A recorded Arena branch, using only its run_start declarations and event evidence. */

type Doc = Record<string, unknown>;
const doc = (v: unknown): Doc => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Doc) : {});
const str = (v: unknown) => (typeof v === 'string' && v ? v : undefined);
const atOf = (e: Event) => { const t = Date.parse(e.at); return Number.isNaN(t) ? 0 : t; };

/** The talk tools' system: Arena's own messages between agents. */
export const TEAM_MESSAGES = 'arena:messages';
const MESSAGES: SystemUnit = { id: TEAM_MESSAGES, kind: 'messaging', name: 'Team messages', product: 'Arena talk tools', basis: 'declared' };
type Declarations = Pick<RunStartPayload, 'systems' | 'tools'>;

function worldOf(b: BranchDetail): Declarations {
  return (b.events.find((e) => e.kind === 'run_start')?.payload ?? {}) as RunStartPayload;
}

function systemOf(part: string, decl: NonNullable<RunStartPayload['systems']>[string]): SystemUnit {
  return {
    id: part, kind: decl.kind, name: decl.product, product: decl.product, part,
    standsInFor: decl.stands_in_for, simulated: decl.simulated, basis: 'declared',
  };
}

function outcomeOf(error: unknown): { outcome: Outcome; reason?: string } {
  if (error == null) return { outcome: 'ok' };
  const reason = typeof error === 'string' ? error : JSON.stringify(error);
  return { outcome: REFUSAL_WORDS.test(reason) ? 'refused' : 'error', reason };
}

function toolAction(e: Event, declarations: Declarations): PlantAction {
  const p = doc(e.payload);
  const tool = str(p.name) ?? 'a tool';
  const declared = declarations.tools?.[tool];
  const part = str(p.part);
  const system = part && declarations.systems?.[part] ? part
    : declared?.part && declarations.systems?.[declared.part] ? declared.part : 'unknown';
  const effect = declared?.effect ?? 'unknown';
  const ms = typeof p.duration_ms === 'number' ? p.duration_ms : undefined;
  return {
    id: 's' + e.seq, at: atOf(e), agent: e.agent ?? 'unknown', system, tool, effect,
    effectBasis: effect === 'unknown' ? 'unknown' : 'declared', ms,
    ...outcomeOf(p.error), request: p.args, result: p.error != null ? p.error : p.result,
  };
}

/** A handoff or message: a write to the team's messages, to one agent or everyone. */
function talkAction(e: Event): PlantAction {
  const p = doc(e.payload);
  const to = str(p.to) ?? (e.kind === 'handoff' ? 'next' : undefined);
  return {
    id: `s${e.seq}`, at: atOf(e), agent: e.agent ?? 'unknown', system: TEAM_MESSAGES, tool: e.kind, effect: 'write', effectBasis: 'declared', outcome: 'ok',
    ...(to ? { to } : {}), request: p, result: null,
  };
}

/** Every agent's actions in recorded order: tool calls, handoffs and messages. */
export function arenaActions(b: BranchDetail, declarations: Declarations): PlantAction[] {
  return [...b.events].sort((x, y) => x.seq - y.seq).flatMap((e) => {
    if (e.kind === 'tool_call') return [toolAction(e, declarations)];
    if (e.kind === 'handoff' || (e.kind as string) === 'message') return [talkAction(e)];
    return [];
  });
}

type ChangeDoc = { id?: unknown; summary?: unknown; action?: unknown; system?: unknown; decision?: unknown; decided_by?: unknown; by?: unknown };

function changesIn(result: unknown): ChangeDoc[] {
  const r = doc(result);
  if (Array.isArray(r.changes)) return r.changes.map(doc);
  return typeof r.id === 'string' ? [r] : [];
}

const GRANTED = /^(approved?|allow(ed)?|granted|yes)$/i;

function permitOf(c: ChangeDoc, at: number, agent: string | undefined): Permit {
  const decision = str(c.decision) ?? str(doc(c.decision).decision);
  const by = str(c.decided_by) ?? str(c.by) ?? str(doc(c.decision).by);
  const state = decision == null && c.decision == null ? 'waiting' : GRANTED.test(decision ?? '') || doc(c.decision).approved === true ? 'granted' : 'refused';
  return {
    id: String(c.id), at, ...(agent ? { agent } : {}), title: str(c.summary) ?? str(c.action) ?? String(c.id), state,
    ...(state !== 'waiting' && by ? { by } : {}), ...(str(c.system) ? { system: str(c.system) } : {}),
  };
}

/** The permits the run's approvals parts showed, each as it stood last; the action that asked for one carries it. */
function permitsOf(actions: PlantAction[], systems: SystemUnit[]): Permit[] {
  const gates = new Set(systems.filter((s) => s.kind === 'approvals').map((s) => s.id));
  const latest = new Map<string, Permit>();
  for (const a of actions) {
    if (!gates.has(a.system) || a.outcome !== 'ok') continue;
    for (const c of changesIn(a.result)) {
      const first = latest.get(String(c.id));
      latest.set(String(c.id), permitOf(c, first?.at ?? a.at, first?.agent ?? a.agent));
      if (!first && changesIn(a.result).length === 1) a.permit = String(c.id);
    }
  }
  return [...latest.values()];
}

/** Read-only agents (Arena records it on the team): their decision rights. */
function rightsOf(b: BranchDetail): Rights {
  const out: Rights = {};
  for (const a of b.team.agents) {
    const ro = (a as { read_only?: unknown }).read_only;
    out[a.id] = ro === true ? ['read only: Arena gives it only read tools'] : ro === false ? ['may use write tools'] : [];
  }
  return out;
}

function usageOf(b: BranchDetail): UsagePoint[] {
  return b.events.filter((e) => e.kind === 'model_call').map((e) => {
    const p = doc(e.payload);
    const u = doc(p.usage);
    const context = typeof u.prompt_tokens === 'number' ? u.prompt_tokens : undefined;
    const tokens = typeof u.total_tokens === 'number' ? u.total_tokens : context !== undefined && typeof u.completion_tokens === 'number' ? context + u.completion_tokens : undefined;
    return { at: atOf(e), ...(e.agent ? { agent: e.agent } : {}), ...(context !== undefined ? { context } : {}), ...(tokens !== undefined ? { tokens } : {}), ...(typeof p.cost_usd === 'number' ? { costUsd: p.cost_usd } : {}) };
  });
}

/** The branch as a plant: its world's systems (and the team's messages when used), actions, permits, rights and usage. */
export function arenaPlant(b: BranchDetail): Plant {
  const declarations = worldOf(b);
  const actions = arenaActions(b, declarations);
  const all = Object.entries(declarations.systems ?? {}).map(([part, decl]) => systemOf(part, decl));
  if (actions.some((a) => a.system === TEAM_MESSAGES)) all.push(MESSAGES);
  if (actions.some((a) => a.system === 'unknown') && !all.some((s) => s.id === 'unknown')) all.push({ id: 'unknown', kind: 'unknown', name: 'Not said', basis: 'unknown' });
  return { systems: all, actions, permits: permitsOf(actions, all), rights: rightsOf(b), usage: usageOf(b) };
}
