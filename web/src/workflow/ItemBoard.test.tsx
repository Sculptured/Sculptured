import { render, screen, within } from '@testing-library/react';
import { expect, it } from 'vitest';
import { ItemBoard } from './ItemBoard';
import type { WorkflowRun } from '../generated/WorkflowRun';
import type { WorkflowTask } from '../generated/WorkflowTask';

const t = (id: string, status: WorkflowTask['status'], depends_on: string[] = []): WorkflowTask =>
  ({ input: { id, title: id.toUpperCase(), context: 'c', depends_on }, stage: 'build', status, assignment: null, history: [], problem: null });
const run: WorkflowRun = { rig: 't', start_seq: 1, batch_id: 'b', summary: 's', assembly: t('assembly', 'queued'),
  config: { input: '', dispatcher: 'd', collector: 'c', entry: 'build', max_tasks: 9, max_steps: 20, stages: [{ id: 'build', name: 'Build', agent: 'b', routes: { ready: null } }] },
  tasks: [t('a', 'complete'), t('b', 'assigned', ['a']), t('c', 'queued', ['b'])] };

it('shows each wave of topics with where it is, in words', () => {
  render(<ItemBoard run={run} usage={{}} now={10} />);
  const list = within(screen.getByRole('list', { name: 'Waves' }));
  expect(list.getAllByRole('listitem').map((li) => li.textContent)).toEqual([
    'Wave 1 · done · 1 of 1: A',
    'Wave 2 · in progress · 0 of 1: B',
    'Wave 3 · waiting for wave 2 · 0 of 1: C',
  ]);
});
