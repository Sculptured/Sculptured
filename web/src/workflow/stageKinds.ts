import type { StageKind } from '../generated/StageKind';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Workflow } from '../generated/Workflow';
import type { WorkflowStage } from '../generated/WorkflowStage';
import { stageOrder } from './itemBoardModel';

/** A stage's type as the editor offers it: the four kinds, plus a review panel (an agent stage with reviewers). */
export type StageType = StageKind | 'panel';

export const STAGE_TYPES: { value: StageType; label: string }[] = [
  { value: 'agent', label: 'Agent' },
  { value: 'check', label: 'Check (Sculptured runs a command)' },
  { value: 'human', label: 'Person decides' },
  { value: 'merge', label: "Merge the item's branch" },
  { value: 'panel', label: 'Review panel' },
];

export function typeOf(s: WorkflowStage): StageType {
  return s.reviewers?.length ? 'panel' : s.kind ?? 'agent';
}

/** Agents a stage may use: everyone except the dispatcher and the assembler. */
function eligible(draft: TeamDraft): string[] {
  const w = draft.workflow!;
  return draft.agents.map((a) => a.id).filter((id) => id !== w.dispatcher && id !== w.collector);
}

/** Drops the fields left undefined, so a stage of one type carries no other type's fields. */
function clean(s: WorkflowStage): WorkflowStage {
  return Object.fromEntries(Object.entries(s).filter(([, v]) => v !== undefined)) as WorkflowStage;
}

/** Where a failed check, a conflict, a rework or a change request goes: the stage before this one in route
 *  order, else this stage again. Never null, which would accept the item. */
function backTo(w: Workflow, id: string): string {
  const order = stageOrder(w), at = order.indexOf(id);
  return order.slice(0, Math.max(at, 0)).reverse().find((p) => Object.values(w.stages.find((s) => s.id === p)!.routes).includes(id)) ?? id;
}

type Builder = (s: WorkflowStage, base: WorkflowStage, forward: string | null, back: string | null, draft: TeamDraft) => WorkflowStage;

const BUILDERS: Record<Exclude<StageType, 'agent'>, Builder> = {
  check: (s, base, forward, back) => clean({ ...base, kind: 'check', command: s.command ?? 'npm test', timeout_s: s.timeout_s, routes: { passed: forward, failed: back } }),
  human: (s, base, forward, back) => clean({ ...base, kind: 'human', prompt: s.prompt ?? 'Approve this item?', routes: { approve: forward, rework: back } }),
  merge: (s, base, forward, back) => clean({ ...base, kind: 'merge', into: s.into, routes: { merged: forward, conflict: back } }),
  panel: (s, base, forward, back, draft) => clean({ ...base, reviewers: s.reviewers?.length ? s.reviewers : eligible(draft).slice(0, 2), join: s.join, priority: s.priority, routes: { approved: forward, changes: back } }),
};

const asAgent: Builder = (s, base, _forward, _back, draft) =>
  clean({ ...base, agent: s.agent || eligible(draft)[0] || '', workers: (s.kind ?? 'agent') === 'agent' && !s.reviewers?.length ? s.workers : undefined });

/** The stage as `type`: that type's required fields filled with defaults, other types' fields cleared. Its
 *  first destination becomes the new type's way forward, so the design keeps its path; its way back never
 *  accepts the item. */
export function asKind(s: WorkflowStage, type: StageType, draft: TeamDraft, back = backTo(draft.workflow!, s.id)): WorkflowStage {
  const forward = Object.values(s.routes).find((to) => to != null) ?? null;
  const base: WorkflowStage = { id: s.id, name: s.name, agent: '', routes: s.routes, max_rounds: s.max_rounds, on_limit: s.on_limit };
  const build = type === 'agent' ? asAgent : BUILDERS[type] ?? asAgent;
  return build(s, base, forward, back, draft);
}

/** A new stage of `type` after `from`: `from`'s first outcome now leads to it, and it leads where that went. */
export function addStageAfter(w: Workflow, from: string, type: StageType, draft: TeamDraft): Workflow {
  let n = 1;
  while (w.stages.some((s) => s.id === `stage-${n}`)) n++;
  const id = `stage-${n}`, source = w.stages.find((s) => s.id === from)!;
  const first = Object.keys(source.routes)[0] ?? 'next';
  const label = STAGE_TYPES.find((t) => t.value === type)!.label.split(' (')[0];
  const added = asKind({ id, name: `New ${label.toLowerCase()} stage`, agent: '', routes: { next: source.routes[first] ?? null } }, type, draft, from);
  return { ...w, stages: [...w.stages.map((s) => (s.id === from ? { ...s, routes: { ...s.routes, [first]: id } } : s)), added] };
}
