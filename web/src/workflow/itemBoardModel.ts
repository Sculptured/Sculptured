import type { CostSource } from '../generated/CostSource';
import type { TaskStatus } from '../generated/TaskStatus';
import type { TurnTokens } from '../generated/TurnTokens';
import type { Workflow } from '../generated/Workflow';
import type { WorkflowRun } from '../generated/WorkflowRun';
import type { WorkflowTask } from '../generated/WorkflowTask';

/** `sinceMs` is null until the item has a first step; `partial` when some of its agents' turns reported no tokens. */
export type Card = { task: string; title: string; stage: string; status: TaskStatus; rounds: number; sinceMs: number | null; tokens: number | null; partial: boolean; badge: string | null; costUsd: number | null; costPartial: boolean; costSources: CostSource[] };
export type Column = { stage: string; name: string; cards: Card[] };
/** `tokensPerDone` averages over the `doneReported` done items that reported tokens. */
export type Summary = { done: number; doneReported: number; waiting: number; blocked: number; tokensPerDone: number | null; costPerDone: number | null; sendBacks: Record<string, number>; waitingMs: number };

/** Whether a workflow uses anything beyond agents handing work along (the item board is for those). */
export function usesPipeline(w: Workflow): boolean {
  return w.copies === 'per-item' || w.stages.some((s) => (s.kind ?? 'agent') !== 'agent' || (s.reviewers?.length ?? 0) > 0 || s.max_rounds != null);
}

/** Stages in route order from the entry (each once), then any the routes never reach, in design order. */
export function stageOrder(w: Workflow): string[] {
  const order: string[] = [];
  const queue = [w.entry];
  while (queue.length) {
    const id = queue.shift()!;
    const s = w.stages.find((x) => x.id === id);
    if (!s || order.includes(id)) continue;
    order.push(id);
    queue.push(...Object.values(s.routes).filter((to): to is string => !!to), ...(s.on_limit ? [s.on_limit] : []));
  }
  return [...order, ...w.stages.map((s) => s.id).filter((id) => !order.includes(id))];
}

/** Every assignment an item has had: its steps', its panels', and the ones in progress. */
function assignments(t: WorkflowTask): string[] {
  return [
    ...t.history.flatMap((h) => [h.assignment, ...(h.panel ?? []).flatMap((r) => (r.assignment ? [r.assignment] : []))]),
    ...(t.assignment ? [t.assignment] : []),
    ...(t.reviews ?? []).flatMap((r) => (r.assignment ? [r.assignment] : [])),
  ];
}

/** Assignments an agent worked on (check, person and Sculptured steps use no tokens). */
function agentAssignments(t: WorkflowTask): string[] {
  const steps = t.history.flatMap((h) => (h.panel?.length ? h.panel.flatMap((r) => (r.assignment ? [r.assignment] : [])) : (h.by ?? 'agent') === 'agent' ? [h.assignment] : []));
  return [...steps, ...(t.assignment ? [t.assignment] : []), ...(t.reviews ?? []).flatMap((r) => (r.assignment ? [r.assignment] : []))];
}

/** CLI-reported input and output tokens over an item's turns (null when none reported), and whether some turns reported none. */
function tokensOf(t: WorkflowTask, usage: Record<string, TurnTokens>): { tokens: number | null; partial: boolean } {
  const reported = assignments(t).flatMap((a) => (usage[a] ? [usage[a].input + usage[a].output] : []));
  return { tokens: reported.length ? reported.reduce((a, b) => a + b, 0) : null, partial: reported.length > 0 && agentAssignments(t).some((a) => !usage[a]) };
}

/** Estimated cost over an item's turns (null when none priced), whether some turns had no estimate, and its sources. */
function costOf(t: WorkflowTask, usage: Record<string, TurnTokens>): { costUsd: number | null; costPartial: boolean; costSources: CostSource[] } {
  const reported = assignments(t).flatMap((a) => (usage[a] ? [usage[a]] : []));
  const priced = reported.filter((u) => u.cost_usd != null);
  return {
    costUsd: priced.length ? priced.reduce((sum, u) => sum + u.cost_usd!, 0) : null,
    costPartial: priced.length > 0 && (priced.length < reported.length || priced.some((u) => u.cost_partial) || agentAssignments(t).some((a) => !usage[a])),
    costSources: [...new Set(priced.flatMap((u) => (u.cost_source ? [u.cost_source] : [])))],
  };
}

function badge(t: WorkflowTask): string | null {
  if (t.status === 'waiting') return 'waiting for a person';
  if (t.status === 'blocked') return `blocked: ${t.problem ?? 'see its steps'}`;
  if (t.history.some((h) => h.by === 'sculptured' && h.outcome === 'merged')) return 'merged';
  const check = [...t.history].reverse().find((h) => h.by === 'check');
  return check ? `check ${check.outcome}` : null;
}

export function itemBoard(run: WorkflowRun, usage: Record<string, TurnTokens>, now: number): { columns: Column[]; summary: Summary } {
  const w = run.config, order = stageOrder(w);
  const card = (t: WorkflowTask): Card => {
    // An item that finished or stopped at this stage is not entering it again.
    const ended = t.status === 'complete' || (t.status === 'blocked' && t.history.at(-1)?.stage === t.stage);
    return {
      task: t.input.id, title: t.input.title, stage: t.stage, status: t.status,
      rounds: t.history.filter((h) => h.stage === t.stage).length + (ended ? 0 : 1),
      sinceMs: t.waiting?.since_ms ?? t.history.at(-1)?.at_ms ?? null,
      ...tokensOf(t, usage), ...costOf(t, usage), badge: badge(t),
    };
  };
  const columns: Column[] = order.map((id) => ({ stage: id, name: w.stages.find((s) => s.id === id)!.name, cards: [] }));
  const done: Column = { stage: '$done', name: 'Done', cards: [] };
  for (const t of run.tasks) {
    if (t.status === 'complete') { done.cards.push(card(t)); continue; }
    let column = columns.find((c) => c.stage === t.stage);
    if (!column) { column = { stage: t.stage, name: `Unknown stage: ${t.stage}`, cards: [] }; columns.push(column); }
    column.cards.push(card(t));
  }
  // A send-back: the item returns to a stage it first reached before the one it leaves (its own path, not the design's order).
  const sendBacks: Record<string, number> = {};
  for (const t of run.tasks) {
    const stages = [...t.history.map((h) => h.stage), t.stage], first = new Map<string, number>();
    stages.forEach((s, i) => {
      const prev = stages[i - 1];
      if (i > 0 && first.has(s) && first.get(s)! < first.get(prev)!) sendBacks[prev] = (sendBacks[prev] ?? 0) + 1;
      if (!first.has(s)) first.set(s, i);
    });
  }
  const doneTokens = done.cards.flatMap((c) => (c.tokens == null ? [] : [c.tokens]));
  const doneCosts = done.cards.flatMap((c) => (c.costUsd == null ? [] : [c.costUsd]));
  const waiting = run.tasks.filter((t) => t.status === 'waiting');
  return {
    columns: [...columns, done],
    summary: {
      done: done.cards.length, doneReported: doneTokens.length, waiting: waiting.length, blocked: run.tasks.filter((t) => t.status === 'blocked').length,
      tokensPerDone: doneTokens.length ? Math.round(doneTokens.reduce((a, b) => a + b, 0) / doneTokens.length) : null,
      costPerDone: doneCosts.length ? doneCosts.reduce((a, b) => a + b, 0) / doneCosts.length : null,
      sendBacks, waitingMs: waiting.reduce((sum, t) => sum + now - (t.waiting?.since_ms ?? now), 0),
    },
  };
}

export type Wave = { wave: number; topics: string[]; done: number; started: boolean };

/** Topics grouped by what they wait for: wave 1 needs nothing, each later wave needs an earlier one
 * (an idea from Gas Town's convoys). A dependency on a topic not in the run is ignored; dependencies
 * are acyclic, and a cycle is cut. Empty when every topic is in one wave. */
export function waves(run: WorkflowRun): Wave[] {
  const ids = new Set(run.tasks.map((t) => t.input.id));
  const level = new Map<string, number>();
  const of = (t: WorkflowTask, seen: Set<string>): number => {
    const known = level.get(t.input.id);
    if (known !== undefined) return known;
    const deps = (t.input.depends_on ?? []).filter((d) => ids.has(d) && !seen.has(d));
    const next = new Set(seen).add(t.input.id);
    const n = 1 + Math.max(0, ...deps.map((d) => of(run.tasks.find((x) => x.input.id === d)!, next)));
    level.set(t.input.id, n);
    return n;
  };
  run.tasks.forEach((t) => of(t, new Set()));
  const count = Math.max(0, ...level.values());
  if (count < 2) return [];
  return Array.from({ length: count }, (_, i) => {
    const in_ = run.tasks.filter((t) => level.get(t.input.id) === i + 1);
    return { wave: i + 1, topics: in_.map((t) => t.input.title), done: in_.filter((t) => t.status === 'complete').length, started: in_.some((t) => t.status !== 'queued') };
  });
}
