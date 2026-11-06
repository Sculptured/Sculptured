import { plural } from '../format';
import { savedParts } from '../tests/restore';
import type { BranchDetail, BranchStatus, Event, RunStartPayload } from '../tests/types';
import { REFUSAL_WORDS } from './alarms';
import { perMinute } from './rate';
import type { AgentLine, Approval, Availability, FeedItem, Tone } from './source';

/**
 * A simulated run (an Arena branch) in the source's terms, from its recorded events: each agent's
 * status, the feed in plain words, the approvals waiting in its world, and its outcome. Pure.
 */

type Payload = Record<string, unknown>;
const payloadOf = (e: Event) => (e.payload ?? {}) as Payload;
const keysOf = (v: unknown): string[] => (Array.isArray(v) ? v.filter((k): k is string => typeof k === 'string') : []);
const atOf = (e: Event) => { const t = Date.parse(e.at); return Number.isNaN(t) ? 0 : t; };

/** An agent's name by id, from the branch's team; its id when the team does not name it. */
export function namesOf(b: BranchDetail): (id: string | null | undefined) => string {
  const names = new Map(b.team.agents.map((a) => [a.id, a.name]));
  return (id) => (id ? names.get(id) ?? id : 'the team');
}

/** A tool call in words: what it used, what it changed, what was saved after it. */
function toolText(who: string, p: Payload): string {
  const tool = typeof p.name === 'string' ? p.name : 'a tool';
  if (p.error != null) return `${who} used ${tool}; the tool refused the call`;
  const wrote = keysOf(p.writes).length;
  const saved = savedParts(p);
  return `${who} used ${tool}${wrote ? ` and changed ${plural(wrote, 'row')}` : ''}${saved.length ? ` · saved ${saved.join(', ')}` : ''}`;
}

/** One recorded event in plain words: "Triage used get_order_details". */
export function eventText(e: Event, who: string): string {
  const p = payloadOf(e);
  switch (e.kind) {
    case 'run_start': return `The run started${typeof p.task_id === 'string' ? ` (task ${p.task_id})` : ''}`;
    case 'agent_start': return `${who} started`;
    case 'model_call': return `${who} called its model`;
    case 'tool_call': return p.snapshot_error ? `${toolText(who, p)}; the world could not be saved after it` : toolText(who, p);
    case 'handoff': return `${who} handed work on`;
    case 'agent_end': return p.ok === false ? `${who} stopped with an error` : `${who} finished (reported)`;
    case 'check': {
      const unsafe = failedSafety(p);
      if (unsafe.length) return `A hard safety check failed: ${unsafe.join(', ')}`;
      return p.passed ? 'The final check passed' : 'The final check failed';
    }
    default: return `${who} recorded ${String(e.kind)}`;
  }
}

/** The hard safety checks (`safety: true`) a check event reports as failed, by name. */
export function failedSafety(p: Payload): string[] {
  const checks = Array.isArray(p.checks) ? (p.checks as Payload[]) : [];
  return checks.filter((c) => c && c.safety === true && c.passed === false).map((c) => String(c.name ?? 'unnamed'));
}

/** The alarm an event raises, with the key of its condition (an agent's end matches its status's alarm). */
function alarmOf(e: Event): Pick<FeedItem, 'alarm' | 'alarmKey'> {
  const p = payloadOf(e);
  if (e.kind === 'agent_end' && p.ok === false && e.agent) return { alarm: 'failed', alarmKey: `end:${e.agent}` };
  if (e.kind === 'tool_call' && p.snapshot_error) return { alarm: 'part-error', alarmKey: `save:${e.seq}` };
  if (e.kind === 'tool_call' && p.error != null && REFUSAL_WORDS.test(String(p.error))) return { alarm: 'refused', alarmKey: `tool:${e.seq}` };
  if (e.kind === 'check' && failedSafety(p).length > 0) return { alarm: 'safety', alarmKey: `safety:${e.branch_id}` };
  return {};
}

/** The branch's events as the feed, oldest first; a run that could not finish adds an alarm for it. */
export function feedOf(b: BranchDetail): FeedItem[] {
  const name = namesOf(b);
  const items: FeedItem[] = [...b.events].sort((x, y) => x.seq - y.seq).map((e) => ({
    id: `s${e.seq}`, at: atOf(e), ...(e.agent ? { agent: e.agent } : {}), kind: e.kind, text: eventText(e, name(e.agent)), raw: e, ...alarmOf(e),
  }));
  if (b.status === 'infra_error') {
    const at = Date.parse(b.ended_at ?? '') || items.at(-1)?.at || Date.parse(b.created_at) || 0;
    items.push({ id: 'run-error', at, kind: 'run-error', text: 'The run could not finish', raw: { error: b.error }, alarm: 'failed', alarmKey: `run:${b.id}` });
  }
  return items;
}

type Status = AgentLine['status'];
const S = (glyph: string, word: string, tone: Tone): Status => ({ glyph, word, tone });

/** What an agent that is still going is doing now, from its latest step. */
function doing(last: Event | undefined): string | undefined {
  if (!last) return undefined;
  const p = payloadOf(last);
  if (last.kind === 'tool_call') return `using ${typeof p.name === 'string' ? p.name : 'a tool'}`;
  if (last.kind === 'model_call') return 'calling its model';
  if (last.kind === 'handoff') return 'handing work on';
  return undefined;
}

/** One agent: not started, working, done (reported), failed, or stopped with the run. */
function agentOf(b: BranchDetail, id: string, name: string): AgentLine {
  const own = b.events.filter((e) => e.agent === id).sort((x, y) => x.seq - y.seq);
  const end = own.find((e) => e.kind === 'agent_end');
  const live = b.status === 'running';
  const base = { id, name };
  if (end) {
    if (payloadOf(end).ok === false) return { ...base, status: S('✕', 'failed', 'bad'), alarm: { rule: 'failed', key: `end:${id}` } };
    return { ...base, status: S('✓', 'done (reported)', 'idle') };
  }
  if (!own.some((e) => e.kind === 'agent_start')) return { ...base, status: live ? S('○', 'not started', 'idle') : S('○', 'did not run', 'idle') };
  if (!live) return { ...base, status: S('■', 'stopped', 'idle') };
  const activity = doing(own.at(-1));
  return { ...base, status: S('⣟', 'working', 'ok'), ...(activity ? { activity } : {}) };
}

/** The team's agents in its order, then any other agent its events name. */
export function agentsOf(b: BranchDetail): AgentLine[] {
  const ids = b.team.agents.map((a) => a.id);
  for (const e of b.events) if (e.agent && !ids.includes(e.agent)) ids.push(e.agent);
  const name = namesOf(b);
  return ids.map((id) => agentOf(b, id, name(id)));
}

const OUTCOME: Record<BranchStatus, Status> = {
  running: S('●', 'running', 'ok'),
  passed: S('✓', 'passed the check', 'idle'),
  failed: S('✕', 'failed the check', 'bad'),
  infra_error: S('⚠', "couldn't run", 'bad'),
  interrupted: S('■', 'stopped', 'idle'),
};

/** The branch's outcome as the header's run status. */
export const outcomeOf = (status: BranchStatus): Status => OUTCOME[status];

export const DECIDED_BY_AGENTS = 'Changes in an Arena run are decided by its agents. To try another decision, fork with changed instructions.';
const NOT_HERE: Availability = { ok: false, why: DECIDED_BY_AGENTS };

/** The parts of the run's world that hold Sculptured's approvals (the `sculptured` integration). */
export function approvalParts(b: BranchDetail): Set<string> {
  const start = b.events.find((e) => e.kind === 'run_start')?.payload as RunStartPayload | undefined;
  return new Set((start?.world ?? []).filter((w) => w.integration === 'sculptured').map((w) => w.part));
}

type ChangeDoc = { id?: unknown; action?: unknown; system?: unknown; summary?: unknown; reason?: unknown; decision?: unknown };

/** The changes a tool call showed: one (propose, decide) or many (list). */
function changesIn(p: Payload): ChangeDoc[] {
  const r = p.result as { changes?: unknown } & ChangeDoc | null | undefined;
  if (!r || typeof r !== 'object') return [];
  if (Array.isArray(r.changes)) return r.changes.filter((c): c is ChangeDoc => !!c && typeof c === 'object');
  return typeof r.id === 'string' ? [r] : [];
}

const str = (v: unknown) => (typeof v === 'string' && v ? v : undefined);

function approvalOf(c: ChangeDoc, agent: string | undefined): Approval {
  const detail = [str(c.action) && `Action: ${str(c.action)}`, str(c.system) && `System: ${str(c.system)}`, str(c.reason) && `Reason: ${str(c.reason)}`].filter(Boolean).join(' · ');
  return { id: String(c.id), ...(agent ? { agent } : {}), title: str(c.summary) ?? str(c.action) ?? String(c.id), ...(detail ? { detail } : {}), kind: 'change', can: NOT_HERE };
}

/**
 * The changes still waiting for a decision at the branch's latest step, from what the world's
 * Sculptured tools showed. None when the world has no Sculptured part.
 */
export function approvalsOf(b: BranchDetail): Approval[] {
  const parts = approvalParts(b);
  if (parts.size === 0) return [];
  const latest = new Map<string, { doc: ChangeDoc; agent?: string }>();
  for (const e of [...b.events].sort((x, y) => x.seq - y.seq)) {
    const p = payloadOf(e);
    if (e.kind !== 'tool_call' || p.error != null || !parts.has(String(p.part))) continue;
    for (const c of changesIn(p)) latest.set(String(c.id), { doc: c, agent: latest.get(String(c.id))?.agent ?? e.agent ?? undefined });
  }
  return [...latest.values()].filter((c) => c.doc.decision == null).map((c) => approvalOf(c.doc, c.agent));
}

/** Tool calls per minute over the last RATE_MINUTES up to `end` (the run's end, or now while it runs), oldest first. */
export function toolCallsPerMin(b: BranchDetail, end: number): (number | null)[] {
  return perMinute(b.events.filter((e) => e.kind === 'tool_call').map(atOf), end);
}
