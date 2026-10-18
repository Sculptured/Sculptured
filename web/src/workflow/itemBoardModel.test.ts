import { expect, it } from 'vitest';
import { itemBoard, stageOrder, usesPipeline, waves } from './itemBoardModel';
import type { Workflow } from '../generated/Workflow';
import type { WorkflowRun } from '../generated/WorkflowRun';
import type { WorkflowTask } from '../generated/WorkflowTask';
import type { WorkflowStep } from '../generated/WorkflowStep';

const config: Workflow = { input: '', dispatcher: 'd', collector: 'c', entry: 'build', max_tasks: 9, max_steps: 20, stages: [
  { id: 'chief', name: 'Sign-off', agent: '', kind: 'human', prompt: 'Ship?', routes: { ship: null, rework: 'build' } },
  { id: 'build', name: 'Build', agent: 'b', routes: { ready: 'tests' } },
  { id: 'tests', name: 'Tests', agent: '', kind: 'check', command: 'npm test', routes: { passed: 'chief', failed: 'build' } },
] };
const step = (assignment: string, stage: string, outcome: string, at_ms: number, by?: WorkflowStep['by']): WorkflowStep => ({ assignment, stage, agent: 'b', outcome, result: 'r', at_ms, by });
const task = (id: string, stage: string, status: WorkflowTask['status'], history: WorkflowStep[], extra: Partial<WorkflowTask> = {}): WorkflowTask =>
  ({ input: { id, title: id.toUpperCase(), context: 'c' }, stage, status, assignment: null, history, problem: null, ...extra });
const run: WorkflowRun = { rig: 't', start_seq: 1, batch_id: 'b', summary: 's', config, assembly: task('assembly', 'assembly', 'queued', []), tasks: [
  task('i1', 'chief', 'waiting', [step('a1', 'build', 'ready', 1), step('a2', 'tests', 'failed', 2, 'check'), step('a3', 'build', 'ready', 3), step('a4', 'tests', 'passed', 4, 'check')], { waiting: { stage: 'chief', since_ms: 5 } }),
  task('i2', 'build', 'blocked', [], { problem: 'no repo' }),
  task('i3', 'gone', 'queued', []),
  task('i4', 'chief', 'complete', [step('b1', 'build', 'ready', 1), step('b2', 'tests', 'passed', 2, 'check'), step('b3', 'chief', 'ship', 3, 'person')]),
  task('i5', 'chief', 'complete', [step('c1', 'build', 'ready', 1), step('c2', 'build', 'ready', 2), step('c3', 'chief', 'ship', 3, 'person')]),
] };
const usage = { a1: { input: 100, output: 10, cache_read: 0, cache_write: 0, cost_usd: null, cost_source: null, cost_partial: false }, a3: { input: 50, output: 5, cache_read: 9, cache_write: 0, cost_usd: null, cost_source: null, cost_partial: false }, b1: { input: 10, output: 0, cache_read: 0, cache_write: 0, cost_usd: null, cost_source: null, cost_partial: false }, c1: { input: 30, output: 0, cache_read: 0, cache_write: 0, cost_usd: null, cost_source: null, cost_partial: false } };

it('orders stages from the entry along routes, each once', () => {
  expect(stageOrder(config)).toEqual(['build', 'tests', 'chief']);
});

it('puts each item in its stage with rounds, time, tokens and a badge in words', () => {
  const { columns, summary } = itemBoard(run, usage, 105);
  expect(columns.map((c) => c.name)).toEqual(['Build', 'Tests', 'Sign-off', 'Unknown stage: gone', 'Done']);
  const i1 = columns[2].cards[0];
  expect([i1.task, i1.rounds, i1.tokens, i1.badge, i1.sinceMs]).toEqual(['i1', 1, 165, 'waiting for a person', 5]);
  expect(columns[0].cards[0]).toMatchObject({ task: 'i2', tokens: null, badge: 'blocked: no repo', sinceMs: null });
  expect(columns[3].cards[0]).toMatchObject({ task: 'i3', sinceMs: null });
  expect(columns[4].cards[0]).toMatchObject({ task: 'i4', tokens: 10, partial: false, rounds: 1 });
  expect(columns[4].cards[1]).toMatchObject({ task: 'i5', tokens: 30, partial: true });
  expect(summary).toEqual({ done: 2, doneReported: 2, waiting: 1, blocked: 1, tokensPerDone: 20, costPerDone: null, sendBacks: { tests: 1 }, waitingMs: 100 });
});

it('is only for workflows that use a pipeline feature', () => {
  const plain: Workflow = { ...config, stages: [{ id: 'build', name: 'Build', agent: 'b', routes: { ready: null } }] };
  expect(usesPipeline(plain)).toBe(false);
  expect(usesPipeline(config)).toBe(true);
  expect(usesPipeline({ ...plain, copies: 'per-item' })).toBe(true);
  expect(usesPipeline({ ...plain, stages: [{ ...plain.stages[0], max_rounds: 2 }] })).toBe(true);
});

it('counts a send-back only when an item returns to a stage it reached earlier', () => {
  const branching: Workflow = { ...config, entry: 'build', stages: [
    { id: 'build', name: 'Build', agent: 'b', routes: { x: 'c', y: 'b2' } },
    { id: 'b2', name: 'B', agent: 'b', routes: { go: 'c' } },
    { id: 'c', name: 'C', agent: 'b', routes: { done: null } },
  ] };
  const r = { ...run, config: branching, tasks: [task('j', 'c', 'assigned', [step('d1', 'build', 'y', 1), step('d2', 'b2', 'go', 2)])] };
  expect(itemBoard(r, {}, 10).summary.sendBacks).toEqual({});
});

it('adds up estimated costs with their sources, and says when some turns had none', () => {
  const priced = (usd: number, source: 'claude-code' | 'openai-list-prices') => ({ input: 1, output: 1, cache_read: 0, cache_write: 0, cost_usd: usd, cost_source: source, cost_partial: false });
  const r = { ...run, tasks: [run.tasks[0], run.tasks[3]] };
  const { columns, summary } = itemBoard(r, { a1: priced(0.25, 'claude-code'), a3: { ...priced(0, 'claude-code'), cost_usd: null, cost_source: null }, b1: priced(1.5, 'openai-list-prices') }, 10);
  expect(columns[2].cards[0]).toMatchObject({ costUsd: 0.25, costPartial: true, costSources: ['claude-code'] });
  expect(columns.at(-1)!.cards[0]).toMatchObject({ costUsd: 1.5, costPartial: false, costSources: ['openai-list-prices'] });
  expect(summary.costPerDone).toBeCloseTo(1.5);
});

it('groups topics into waves by what they wait for, with each wave\'s progress (an idea from Gas Town\'s convoys)', () => {
  const t = (id: string, status: WorkflowTask['status'], depends_on: string[] = []) => task(id, 'build', status, [], { input: { id, title: id.toUpperCase(), context: 'c', depends_on } });
  const r: WorkflowRun = { ...run, tasks: [t('a', 'complete'), t('b', 'complete'), t('c', 'assigned', ['a']), t('d', 'queued', ['a', 'c']), t('e', 'queued', ['missing'])] };
  expect(waves(r)).toEqual([
    { wave: 1, topics: ['A', 'B', 'E'], done: 2, started: true },
    { wave: 2, topics: ['C'], done: 0, started: true },
    { wave: 3, topics: ['D'], done: 0, started: false },
  ]);
  expect(waves({ ...run, tasks: [t('x', 'queued'), t('y', 'queued')] }), 'independent topics are one wave, so nothing to draw').toEqual([]);
});
