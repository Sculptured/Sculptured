import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { api } from '../api';
import { draftAgent, teamDraft } from '../test-fixtures';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Workflow } from '../generated/Workflow';
import { DispatchNotice } from './DispatchCommand';
import { WorkflowSettings } from './WorkflowBoard';
import { WorkflowGraph, workflowLinks } from './WorkflowGraph';

vi.mock('../api', async orig => { const m = await orig<typeof import('../api')>(); return { ...m, api: { ...m.api, retryDispatch: vi.fn().mockResolvedValue({ retried: true }) } }; });

const workflow: Workflow = { input: '', dispatcher: 'dispatch', collector: 'frontend', entry: 'b', max_tasks: 4, max_steps: 12, stages: [{ id: 'b', name: 'Build', agent: 'build', routes: { ready: null } }] };
const byCommand: Workflow = { ...workflow, dispatcher: '', dispatch: { command: 'cat tasks.json' } };
const draft = (w: Workflow) => teamDraft({ agents: ['dispatch', 'build', 'frontend'].map((id, i) => draftAgent(id, { x: 40 + i * 260, y: 40 })), connections: [], workflow: w });

let latest: TeamDraft;
function Settings() {
  const [d, set] = useState(draft(workflow));
  latest = d;
  return <WorkflowSettings draft={d} locked={false} change={(w) => set({ ...d, workflow: w })} expanded />;
}

it('sets a dispatch command in Advanced, which takes the dispatcher agent’s place', async () => {
  const user = userEvent.setup();
  render(<Settings />);
  await user.click(screen.getByText('Advanced'));
  await user.type(screen.getByLabelText('Dispatch command'), 'cat x');
  expect(latest.workflow).toMatchObject({ dispatcher: '', dispatch: { command: 'cat x' } });
  expect(screen.getByLabelText('Dispatcher agent')).toBeDisabled();
  expect(screen.getByText(/makes the work, so no agent dispatches it/)).toBeInTheDocument();
  await user.type(screen.getByLabelText('Dispatch command time limit in seconds'), '90');
  expect(latest.workflow!.dispatch).toEqual({ command: 'cat x', timeout_s: 90 });
  await user.clear(screen.getByLabelText('Dispatch command'));
  expect(latest.workflow!.dispatch).toBeUndefined();
  expect(screen.getByLabelText('Dispatcher agent')).toBeEnabled();
});

it('draws a Dispatch command box where the dispatcher would hand out work, with no Input', async () => {
  const open = vi.fn();
  render(<div><WorkflowGraph draft={draft(byCommand)} onOpen={open} onReveal={() => {}} /></div>);
  await userEvent.setup().click(screen.getByRole('button', { name: /^Dispatch command/ }));
  expect(open).toHaveBeenCalledWith('settings');
  expect(screen.getByRole('button', { name: /^Dispatch command/ })).toHaveTextContent('$ cat tasks.json');
  expect(screen.queryByRole('button', { name: /^Input/ })).toBeNull();
  expect(workflowLinks(draft(byCommand)).map((l) => [l.id, l.from])).toEqual([['dispatch', '$dispatch-command'], ['b:ready', 'build']]);
});

it('says plainly when the dispatch command made no work, and runs it again on request', async () => {
  const onRetried = vi.fn();
  const record = { command: 'cat tasks.json', at_ms: 1, dispatched: false, evidence: '$ cat tasks.json\nexit 1 · 3 ms', problem: 'The dispatch command failed (exit 1); nothing was dispatched.' };
  render(<DispatchNotice team="t" start={7} record={record} running={false} active onRetried={onRetried} />);
  expect(screen.getByRole('status')).toHaveTextContent('▲ Blocked: the dispatch command made no work');
  expect(screen.getByText(/exit 1\); nothing was dispatched/)).toHaveAttribute('data-raw-evidence');
  await userEvent.setup().click(screen.getByRole('button', { name: 'Run the dispatch command again' }));
  expect(api.retryDispatch).toHaveBeenCalledWith('t', 7);
  expect(onRetried).toHaveBeenCalled();
});

it('labels work made by the command as Sculptured running it, with no retry', () => {
  render(<DispatchNotice team="t" start={7} record={{ command: 'c', at_ms: 1, dispatched: true, evidence: 'exit 0' }} running={false} active onRetried={() => {}} />);
  expect(screen.getByRole('status')).toHaveTextContent('✓ Work made by the dispatch command');
  expect(screen.getByText(/Observed: Sculptured ran the command; the items are what it printed/)).toBeInTheDocument();
  expect(screen.queryByRole('button')).toBeNull();
});
