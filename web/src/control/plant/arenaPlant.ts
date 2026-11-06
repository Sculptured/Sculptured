import { REFUSAL_WORDS } from '../alarms';
import type { BranchDetail, Event, RunStartPayload, WorldPartInfo } from '../../tests/types';
import type { Basis, Effect, Outcome, Permit, Plant, PlantAction, Rights, SystemUnit, UsagePoint } from './model';

/**
 * An Arena run branch as a plant. Which system an action touched comes from Arena only: the part a
 * tool ran on, the part's `describe.system` ({kind, product, stands_in_for?, simulated}) and each
 * tool's `effect` as Arena keeps them with the branch's world (or, for the benchmark Arena runs now,
 * `GET /api/world`, used only for a part whose integration is the same). Without them the kind and
 * the effect are unknown; rows a call wrote are shown as an observed write. Pure.
 */

type Doc = Record<string, unknown>;
const doc = (v: unknown): Doc => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Doc) : {});
const str = (v: unknown) => (typeof v === 'string' && v ? v : undefined);
const atOf = (e: Event) => { const t = Date.parse(e.at); return Number.isNaN(t) ? 0 : t; };

/** The talk tools' system: Arena's own messages between agents. */
export const TEAM_MESSAGES = 'arena:messages';
const MESSAGES: SystemUnit = { id: TEAM_MESSAGES, kind: 'messaging', name: 'Team messages', product: 'Arena talk tools', basis: 'declared' };
const TALK_TOOLS = new Set(['handoff', 'message', 'inbox']);

/** One part as some source declares it: the run's world, the branch's world info, or `/api/world`. */
type PartDecl = { part: string; integration?: string; system?: Doc; tools: unknown[] };

function declsOf(b: BranchDetail, parts: WorldPartInfo[] | null): { run: PartDecl[]; now: PartDecl[] } {
  const start = b.events.find((e) => e.kind === 'run_start')?.payload as RunStartPayload | undefined;
  const fromList = (list: unknown, key: 'part' | 'name'): PartDecl[] => (Array.isArray(list) ? list : []).flatMap((w) => {
    const d = doc(w);
    const part = str(d[key]) ?? str(d.part) ?? str(d.name);
    return part ? [{ part, integration: str(d.integration), system: d.system ? doc(d.system) : undefined, tools: Array.isArray(d.tools) ? d.tools : [] }] : [];
  });
  const own = doc(b as unknown).world;
  return { run: [...fromList(start?.world, 'part'), ...fromList(own, 'part')], now: fromList(parts, 'name') };
}

function systemOf(part: string, integration: string | undefined, decl: Doc | undefined): SystemUnit {
  if (decl && str(decl.kind)) {
    const product = str(decl.product);
    return {
      id: part, kind: str(decl.kind)!, name: product ?? part, part, ...(product ? { product } : {}),
      ...(str(decl.stands_in_for) ? { standsInFor: str(decl.stands_in_for) } : {}),
      ...(typeof decl.simulated === 'boolean' ? { simulated: decl.simulated } : {}),
      basis: 'declared',
    };
  }
  // Sculptured's own approvals integration: Sculptured knows what it is.
  if (integration === 'sculptured') return { id: part, kind: 'approvals', name: 'Sculptured approvals', part, basis: 'sculptured' };
  return { id: part, kind: 'unknown', name: part, part, basis: 'unknown' };
}

/** The run's parts as systems, each with the declaration that applies to it; then the tools' effects. */
function worldOf(b: BranchDetail, parts: WorldPartInfo[] | null) {
  const { run, now } = declsOf(b, parts);
  const byPart = new Map<string, PartDecl>();
  for (const d of run) byPart.set(d.part, { ...byPart.get(d.part), ...d, system: d.system ?? byPart.get(d.part)?.system, tools: d.tools.length ? d.tools : byPart.get(d.part)?.tools ?? [] });
  // Today's world describes the run's part only when it runs the same integration.
  for (const d of now) {
    const mine = byPart.get(d.part);
    if (mine && mine.integration && d.integration && mine.integration !== d.integration) continue;
    byPart.set(d.part, { part: d.part, integration: mine?.integration ?? d.integration, system: mine?.system ?? d.system, tools: mine?.tools.length ? mine.tools : d.tools });
  }
  const effects = new Map<string, Effect>();
  for (const d of byPart.values()) for (const t of d.tools) {
    const td = doc(t);
    if (str(td.name) && (td.effect === 'read' || td.effect === 'write')) effects.set(td.name as string, td.effect);
  }
  const systems = [...byPart.values()].map((d) => systemOf(d.part, d.integration, d.system));
  return { systems, effects, byPart };
}

const keysOf = (v: unknown) => (Array.isArray(v) ? v.length : 0);

function outcomeOf(error: unknown): { outcome: Outcome; reason?: string } {
  if (error == null) return { outcome: 'ok' };
  const reason = typeof error === 'string' ? error : JSON.stringify(error);
  return { outcome: REFUSAL_WORDS.test(reason) ? 'refused' : 'error', reason };
}

function effectOf(tool: string, p: Doc, effects: Map<string, Effect>): { effect: Effect; effectBasis: Basis } {
  const declared = effects.get(tool);
  if (declared) return { effect: declared, effectBasis: 'declared' };
  if (keysOf(p.writes) > 0) return { effect: 'write', effectBasis: 'observed' };
  return { effect: 'unknown', effectBasis: 'unknown' };
}

function toolAction(e: Event, effects: Map<string, Effect>): PlantAction {
  const p = doc(e.payload);
  const tool = str(p.name) ?? 'a tool';
  const part = str(p.part);
  const system = part ?? (TALK_TOOLS.has(tool) ? TEAM_MESSAGES : 'unknown');
  const talkRead = system === TEAM_MESSAGES && tool === 'inbox';
  const fx = talkRead ? { effect: 'read' as const, effectBasis: 'declared' as const } : effectOf(tool, p, effects);
  return { id: `s${e.seq}`, at: atOf(e), agent: e.agent ?? 'unknown', system, tool, ...fx, ...outcomeOf(p.error), request: p.args, result: p.error != null ? p.error : p.result };
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
export function arenaActions(b: BranchDetail, effects: Map<string, Effect>): PlantAction[] {
  return [...b.events].sort((x, y) => x.seq - y.seq).flatMap((e) => {
    if (e.kind === 'tool_call') return [toolAction(e, effects)];
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
export function arenaPlant(b: BranchDetail, parts: WorldPartInfo[] | null): Plant {
  const { systems, effects } = worldOf(b, parts);
  const actions = arenaActions(b, effects);
  const all = [...systems];
  if (actions.some((a) => a.system === TEAM_MESSAGES)) all.push(MESSAGES);
  if (actions.some((a) => a.system === 'unknown') && !all.some((s) => s.id === 'unknown')) all.push({ id: 'unknown', kind: 'unknown', name: 'Not said', basis: 'unknown' });
  return { systems: all, actions, permits: permitsOf(actions, all), rights: rightsOf(b), usage: usageOf(b) };
}
