import { LOCAL_COST_SOURCE, providerId, providerOf } from './providers';
import { savedParts } from './restore';
import type { AgentKind, AgentSpec, Branch, Change, Event, Team } from './types';

/** What a step was, for its glyph (shape) and word; colour only repeats it. */
export type StepKind = 'start' | 'model' | 'read' | 'write' | 'handoff' | 'end' | 'check';

export interface Step {
  seq: number;
  kind: StepKind;
  label: string;
  /** Milliseconds from the branch's first event: when the work behind this step began and when it was recorded. */
  from: number;
  to: number;
  /** Recorded before this branch's fork point: reused from the parent, not run again. */
  inherited: boolean;
  failed: boolean;
  /** A step after which some part was saved (the branch can restart just after it). */
  snapshot: boolean;
}

export interface Lane {
  agent: string;
  steps: Step[];
}

export interface Layout {
  lanes: Lane[];
  /** Milliseconds the whole branch spans (at least 1). */
  span: number;
  /** Where this branch was forked, in the same milliseconds, or null for an original run. */
  forkAt: number | null;
}

export const CHECK_LANE = 'check';

const keysOf = (v: unknown): string[] => (Array.isArray(v) ? v.filter((k): k is string => typeof k === 'string') : []);

function stepOf(e: Event): Pick<Step, 'kind' | 'label' | 'failed' | 'snapshot'> {
  const p = (e.payload ?? {}) as Record<string, unknown>;
  switch (e.kind) {
    case 'agent_start': return { kind: 'start', label: 'Starts', failed: false, snapshot: false };
    case 'agent_end': return { kind: 'end', label: p.ok === false ? 'Stopped with an error' : 'Finishes', failed: p.ok === false, snapshot: false };
    case 'model_call': return { kind: 'model', label: 'Model call', failed: false, snapshot: false };
    case 'handoff': return { kind: 'handoff', label: 'Hands work on', failed: false, snapshot: false };
    case 'check': return { kind: 'check', label: p.passed ? 'Check passed' : 'Check failed', failed: !p.passed, snapshot: false };
    case 'tool_call': {
      const wrote = keysOf(p.writes).length > 0 && p.error == null;
      return {
        kind: wrote ? 'write' : 'read',
        label: typeof p.name === 'string' ? p.name : 'tool',
        failed: p.error != null,
        snapshot: savedParts(p).length > 0,
      };
    }
    default: return { kind: 'start', label: e.kind, failed: false, snapshot: false };
  }
}

/**
 * One lane per agent in team order, then the check. Each step spans from the previous event in its
 * lane (or its agent's start) to when it was recorded, so a block's width is the time that went
 * into it. Steps before `startSeq` are inherited from the parent branch.
 */
export function layout(events: Event[], team: Team, startSeq: number | null = null): Layout {
  const sorted = [...events].filter((e) => e.kind !== 'run_start').sort((a, b) => a.seq - b.seq);
  const t = (e: Event) => Date.parse(e.at);
  const origin = sorted.length ? Math.min(...sorted.map(t).filter((x) => !Number.isNaN(x))) : 0;
  const at = (e: Event) => { const x = t(e); return Number.isNaN(x) ? 0 : x - origin; };
  const byAgent = new Map<string, Step[]>(team.agents.map((a) => [a.id, []]));
  const checks: Step[] = [];
  let last = 0;
  for (const e of sorted) {
    const lane = e.kind === 'check' || e.agent == null ? checks : (byAgent.get(e.agent) ?? byAgent.set(e.agent, []).get(e.agent)!);
    const prev = lane.length ? lane[lane.length - 1].to : e.kind === 'check' ? last : at(e);
    const to = Math.max(at(e), prev);
    lane.push({ seq: e.seq, ...stepOf(e), from: prev, to, inherited: startSeq != null && e.seq < startSeq });
    last = Math.max(last, to);
  }
  const lanes: Lane[] = [...byAgent].map(([agent, steps]) => ({ agent, steps }));
  if (checks.length) lanes.push({ agent: CHECK_LANE, steps: checks });
  const fork = startSeq == null ? null : sorted.find((e) => e.seq >= startSeq);
  return { lanes, span: Math.max(1, last), forkAt: fork ? at(fork) : null };
}

/** Steps in the order they happened, for the list view and arrow-key moves. */
export const inOrder = (l: Layout): Step[] => l.lanes.flatMap((x) => x.steps).sort((a, b) => a.seq - b.seq);

/** The same lanes with one equal slot per step in order (`span` = number of steps), so every label has room. */
export function bySteps(l: Layout): Layout {
  const order = inOrder(l);
  const slot = new Map(order.map((s, i) => [s.seq, i]));
  const firstOwn = order.findIndex((s) => !s.inherited);
  return {
    lanes: l.lanes.map((x) => ({ ...x, steps: x.steps.map((s) => ({ ...s, from: slot.get(s.seq)!, to: slot.get(s.seq)! + 1 })) })),
    span: Math.max(1, order.length),
    forkAt: l.forkAt == null ? null : firstOwn < 0 ? order.length : firstOwn,
  };
}

/** A few round tick marks for a time axis of `span` ms: [ms, label]. */
export function ticks(span: number): [number, string][] {
  const s = span / 1000;
  const step = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600].find((x) => s / x <= 6) ?? 1200;
  const out: [number, string][] = [];
  for (let v = 0; v <= s + 1e-9; v += step) out.push([v * 1000, v >= 60 ? `${Math.floor(v / 60)}m${v % 60 ? ` ${v % 60}s` : ''}` : `${v}s`]);
  return out;
}

/** Where a cost figure came from, in plain words (provenance). */
export function costSource(source: string | null | undefined): string {
  if (source === 'api_equivalent_estimate') return 'API-equivalent estimate';
  if (source === 'openrouter') return 'reported by OpenRouter';
  if (source === LOCAL_COST_SOURCE) return 'no API charge (local server)';
  if (source === 'mixed') return 'mixed sources';
  return '';
}

export function dollars(usd: number): string {
  return `$${usd.toFixed(usd < 0.1 ? 4 : 2)}`;
}

export interface Summary {
  seconds: number | null;
  live: boolean;
  tokens: number;
  /** Null when any agent has no dollar figure (Codex reports tokens only). */
  cost_usd: number | null;
  cost_source: string | null;
  /** Distinct rows this branch wrote itself. */
  rows_written: number;
  steps: number;
}

/** How long the branch ran (until now while it runs), and whether it still is. */
function branchTiming(branch: Pick<Branch, 'created_at' | 'ended_at' | 'status'>, now: number): { seconds: number | null; live: boolean } {
  const start = Date.parse(branch.created_at);
  const live = !branch.ended_at && branch.status === 'running';
  const end = branch.ended_at ? Date.parse(branch.ended_at) : live ? now : NaN;
  return { seconds: Number.isNaN(start) || Number.isNaN(end) ? null : (end - start) / 1000, live };
}

/** Same arithmetic as arenad's compare: this branch's own events only (seq >= start_seq). */
export function summarize(branch: Pick<Branch, 'start_seq' | 'created_at' | 'ended_at' | 'status'>, events: Event[], now: number = Date.now()): Summary {
  const from = branch.start_seq ?? 0;
  let tokens = 0;
  let cost: number | null = 0;
  let ended = 0;
  const sources: string[] = [];
  const written = new Set<string>();
  let steps = 0;
  for (const e of events) {
    if (e.seq < from) continue;
    const p = (e.payload ?? {}) as Record<string, unknown>;
    if (e.kind === 'tool_call' || e.kind === 'model_call' || e.kind === 'handoff') steps++;
    if (e.kind === 'tool_call' && p.error == null) keysOf(p.writes).forEach((k) => written.add(k));
    if (e.kind !== 'agent_end') continue;
    ended++;
    tokens += typeof p.tokens === 'number' ? p.tokens : 0;
    cost = cost != null && typeof p.cost_usd === 'number' ? cost + p.cost_usd : null;
    if (typeof p.cost_source === 'string' && !sources.includes(p.cost_source)) sources.push(p.cost_source);
  }
  return {
    ...branchTiming(branch, now), tokens,
    cost_usd: ended === 0 ? null : cost,
    cost_source: sources.length === 0 ? null : sources.length === 1 ? sources[0] : 'mixed',
    rows_written: written.size,
    steps,
  };
}

/** Branches in tree order with their depth (parent before children, oldest first). */
export function branchTree<B extends Pick<Branch, 'id' | 'parent_id' | 'created_at'>>(branches: B[]): { branch: B; depth: number }[] {
  const ids = new Set(branches.map((b) => b.id));
  const kids = new Map<string | null, B[]>();
  for (const b of [...branches].sort((x, y) => x.created_at.localeCompare(y.created_at))) {
    const key = b.parent_id && ids.has(b.parent_id) ? b.parent_id : null;
    kids.set(key, [...(kids.get(key) ?? []), b]);
  }
  const out: { branch: B; depth: number }[] = [];
  const walk = (key: string | null, depth: number) => {
    for (const b of kids.get(key) ?? []) {
      out.push({ branch: b, depth });
      walk(b.id, depth + 1);
    }
  };
  walk(null, 0);
  return out;
}

/** A branch's short name: "Original" for the first, else "Rerun N" in creation order. */
export function branchNames(branches: Pick<Branch, 'id' | 'parent_id' | 'created_at'>[]): Map<string, string> {
  const sorted = [...branches].sort((x, y) => x.created_at.localeCompare(y.created_at));
  let n = 0;
  return new Map(sorted.map((b) => [b.id, b.parent_id == null ? 'Original' : `Rerun ${++n}`]));
}

/**
 * Only fields that really changed. Prompts compare trimmed. A cleared model that was set is sent as ""
 * (explicit clear); blank on an unset model is no change. A mode change resets the model in arenad,
 * so a blank model next to it sends just the mode. A provider (API mode only) is sent only when it
 * differs from the agent's (absent is `openrouter`), never blank, and always with its model: the old
 * model belongs to the old provider.
 */
export function changedFields(agents: AgentSpec[], edits: Record<string, Change>): Record<string, Change> {
  const out: Record<string, Change> = {};
  for (const a of agents) {
    const e = edits[a.id];
    if (!e) continue;
    const c: Change = {};
    if (e.prompt != null && e.prompt.trim() !== a.prompt.trim()) c.prompt = e.prompt.trim();
    const kindChanged = e.kind != null && e.kind !== (a.kind as AgentKind);
    const providerChanged = (e.kind ?? a.kind) === 'open' && e.provider != null && providerId(e.provider) !== providerOf(a);
    const m = e.model?.trim();
    if (providerChanged) {
      c.provider = providerId(e.provider);
      c.model = m ?? '';
    } else if (m != null && m !== (a.model ?? '') && (m !== '' || !kindChanged)) c.model = m;
    if (kindChanged) c.kind = e.kind;
    if (Object.keys(c).length) out[a.id] = c;
  }
  return out;
}

export const MAX_ROW_KEYS = 50;

/** Row keys to fetch for a step: writes first, whole-table reads (`table/*`) dropped, capped. */
export function rowKeys(writes: string[], reads: string[]): { keys: string[]; more: number } {
  const all = [...new Set([...writes, ...reads])].filter((k) => !k.endsWith('/*'));
  return { keys: all.slice(0, MAX_ROW_KEYS), more: Math.max(0, all.length - MAX_ROW_KEYS) };
}

/** The step just before `seq` on this branch, where "before" rows are read from; null for the first. */
export function stepBefore(events: Event[], seq: number): number | null {
  let best: number | null = null;
  for (const e of events) if (e.seq < seq && (best == null || e.seq > best)) best = e.seq;
  return best;
}

/** Line-based diff of two pretty-printed JSON values (LCS). */
export interface DiffLine { op: ' ' | '+' | '-'; text: string }
export function jsonDiff(a: unknown, b: unknown): DiffLine[] {
  const fmt = (v: unknown) => (v === undefined ? ['(absent)'] : JSON.stringify(v ?? null, null, 2).split('\n'));
  const x = fmt(a), y = fmt(b);
  const n = x.length, m = y.length;
  const t: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--) t[i][j] = x[i] === y[j] ? t[i + 1][j + 1] + 1 : Math.max(t[i + 1][j], t[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) { out.push({ op: ' ', text: x[i] }); i++; j++; }
    else if (t[i + 1][j] >= t[i][j + 1]) out.push({ op: '-', text: x[i++] });
    else out.push({ op: '+', text: y[j++] });
  }
  while (i < n) out.push({ op: '-', text: x[i++] });
  while (j < m) out.push({ op: '+', text: y[j++] });
  return out;
}

/** Only the changed lines of a diff, with `context` unchanged lines around each change. */
export function diffHunks(lines: DiffLine[], context = 2): (DiffLine | null)[] {
  const keep = lines.map(() => false);
  lines.forEach((l, i) => {
    if (l.op === ' ') return;
    for (let k = Math.max(0, i - context); k <= Math.min(lines.length - 1, i + context); k++) keep[k] = true;
  });
  const out: (DiffLine | null)[] = [];
  lines.forEach((l, i) => {
    if (keep[i]) out.push(l);
    else if (out.length && out[out.length - 1] !== null) out.push(null);
  });
  if (out[out.length - 1] === null) out.pop();
  return out;
}
