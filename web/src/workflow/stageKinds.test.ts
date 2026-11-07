import { expect, it } from 'vitest';
import { addStageAfter, asKind } from './stageKinds';
import { draftAgent, teamDraft } from '../test-fixtures';
import type { Workflow } from '../generated/Workflow';
import type { WorkflowStage } from '../generated/WorkflowStage';

const workflow: Workflow = { input: '', dispatcher: 'dispatch', collector: 'frontend', entry: 'b', max_tasks: 4, max_steps: 12, stages: [
  { id: 'b', name: 'Build', agent: 'build', workers: ['bob'], routes: { ready: 'next' } },
  { id: 'next', name: 'Next', agent: 'alice', routes: { done: null, again: 'b' } },
] };
const draft = teamDraft({ agents: ['dispatch', 'build', 'alice', 'bob', 'frontend'].map((id) => draftAgent(id)), workflow });
const build = workflow.stages[0];
type Parts = { stage: WorkflowStage; panel: boolean; none: boolean; keys: string };

/** One rule per kind: its fields are present and the other kinds' are absent. */
const KIND_RULES: Record<string, (p: Parts) => boolean> = {
  check: ({ stage: s, none, keys }) => none && !!s.command && keys === 'failed,passed' && !s.prompt && !s.into,
  human: ({ stage: s, none }) => none && !!s.prompt && Object.keys(s.routes).length >= 2 && !s.command,
  merge: ({ stage: s, none, keys }) => none && keys === 'conflict,merged' && !s.command && !s.prompt,
  panel: ({ stage: s }) => !s.agent && !s.workers?.length && s.reviewers!.length >= 2 && !s.command && !s.prompt,
  agent: ({ stage: s }) => !!s.agent && !s.command && !s.prompt && !s.into && !s.reviewers?.length,
};

/** The kind rules the server checks (rigspec): each kind's fields present, other kinds' absent. */
function valid(s: WorkflowStage): boolean {
  const panel = (s.reviewers?.length ?? 0) > 0, none = !s.agent && !s.workers?.length && !panel;
  const kind = s.kind ?? 'agent';
  // A stage with reviewers is a panel whatever its kind says, unless its kind has rules of its own.
  const rule = KIND_RULES[kind === 'agent' && panel ? 'panel' : kind];
  return rule({ stage: s, panel, none, keys: Object.keys(s.routes).sort().join(',') });
}

it('turns a stage into each type with that type\'s fields, keeping its way forward', () => {
  for (const kind of ['check', 'human', 'merge', 'panel', 'agent'] as const) {
    const s = asKind(build, kind, draft);
    expect(valid(s), `${kind}: ${JSON.stringify(s)}`).toBe(true);
    expect(Object.values(s.routes)).toContain('next');
  }
  expect(asKind(build, 'panel', draft).reviewers).toEqual(['build', 'alice']);
});

it('gives a stage a valid agent again after it was another type', () => {
  const back = asKind(asKind(build, 'check', draft), 'agent', draft);
  expect(valid(back)).toBe(true);
  expect(back.agent).toBe('build');
});

it('adds a stage after another, routing its first outcome through it', () => {
  const w = addStageAfter(workflow, 'b', 'human', draft);
  const added = w.stages.find((s) => s.id !== 'b' && s.id !== 'next')!;
  expect(w.stages.find((s) => s.id === 'b')!.routes.ready).toBe(added.id);
  expect(added.kind).toBe('human');
  expect(Object.values(added.routes)).toContain('next');
});

it('never lets a failure, conflict, rework or change request accept the item', () => {
  const failure = { check: 'failed', human: 'rework', merge: 'conflict', panel: 'changes' } as const;
  for (const [kind, outcome] of Object.entries(failure)) {
    expect(asKind(workflow.stages[1], kind as keyof typeof failure, draft).routes[outcome], kind).toBe('b');
  }
  const w = addStageAfter(workflow, 'b', 'check', draft);
  expect(w.stages.at(-1)!.routes.failed).toBe('b');
  const bare = { ...workflow, stages: [{ ...workflow.stages[0], routes: {} }, workflow.stages[1]] };
  expect(Object.keys(addStageAfter(bare, 'b', 'human', draft).stages[0].routes)).not.toContain('undefined');
});
