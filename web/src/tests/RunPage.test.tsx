import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { openRigWords } from '../test-vocab';
import { RunPage } from './RunPage';
import type { BranchDetail, Event, EventKind, RunSummary } from './types';

const at = (s: number) => new Date(Date.parse('2026-10-30T10:00:00Z') + s * 1000).toISOString();
const ev = (seq: number, t: number, agent: string | null, kind: EventKind, payload: unknown = {}): Event =>
  ({ branch_id: 'b1', seq, at: at(t), agent, kind, payload });

const branch: BranchDetail = {
  id: 'b1', run_id: 'r1', parent_id: null, start_seq: null, changes: null, fork_note: null,
  status: 'passed', error: null, created_at: at(0), ended_at: at(20),
  team: {
    id: 't', name: 't',
    agents: [
      { id: 'triage', name: 'Triage', kind: 'claude_code', model: null, prompt: 'Look things up.' },
      { id: 'resolver', name: 'Resolver', kind: 'codex', model: null, prompt: '' },
    ],
  },
  events: [
    ev(0, 0, null, 'run_start', { task_id: '27' }),
    ev(1, 0, 'triage', 'agent_start'),
    ev(2, 4, 'triage', 'tool_call', { name: 'get_order_details', args: { order_id: '#W1' }, result: {}, reads: ['orders/#W1'], writes: [] }),
    ev(3, 6, 'triage', 'handoff', { note: 'Swap the boots.' }),
    ev(4, 7, 'triage', 'agent_end', { ok: true, tokens: 100, cost_usd: 0.05, cost_source: 'api_equivalent_estimate', seconds: 7 }),
    ev(5, 7, 'resolver', 'agent_start'),
    ev(6, 15, 'resolver', 'tool_call', { name: 'exchange_delivered_order_items', args: {}, result: {}, reads: [], writes: ['orders/#W1'], snapshot: 's1' }),
    ev(7, 18, 'resolver', 'agent_end', { ok: true, tokens: 200, cost_usd: null, cost_source: null, seconds: 11 }),
    ev(8, 19, null, 'check', { passed: true, gold_db: 'g', differing_rows: [] }),
  ],
};
const runs: RunSummary[] = [{ id: 'r1', task_id: '27', created_at: at(0), branches: [branch] }];

function answer(url: string): unknown {
  if (url.endsWith('/health')) return { claude: true, codex: true, open_available: false };
  if (url.endsWith('/tasks')) return [{ id: '27', reason: 'Exchange the hiking boots.' }];
  if (url.endsWith('/runs')) return runs;
  if (url.includes('/rows?seq=5')) return { seq: 5, rows: { 'orders/#W1': { status: 'delivered' } } };
  if (url.includes('/rows?seq=6')) return { seq: 6, rows: { 'orders/#W1': { status: 'exchange requested' } } };
  if (url.endsWith('/branches/b1')) return branch;
  throw new Error(`unexpected ${url}`);
}

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(async (url: string) => new Response(JSON.stringify(answer(url)), { status: 200 })));
});
afterEach(() => vi.unstubAllGlobals());

const page = (path = '/tests/runs/r1/b1') => render(
  <MemoryRouter initialEntries={[path]}>
    <Routes><Route path="/tests/runs/:run/:branch" element={<RunPage />} /></Routes>
  </MemoryRouter>,
);

describe('RunPage', () => {
  it('shows the task, its outcome as shape and word, and honest totals', async () => {
    const { container } = page();
    expect(await screen.findByRole('heading', { name: /Task 27/ })).toBeInTheDocument();
    expect(screen.getAllByText('Passed the check')[0]).toHaveTextContent('✓Passed the check');
    // Codex gave no dollar figure, so the run has none.
    expect(screen.getByText('tokens only')).toBeInTheDocument();
    expect(screen.getByText('300')).toBeInTheDocument();
    expect(openRigWords(container)).toEqual([]);
  });

  it('a block opens its step; a write shows the row before and after', async () => {
    page();
    const lane = await screen.findByRole('group', { name: 'Resolver steps' });
    await userEvent.click(within(lane).getByRole('button', { name: /^Step 6: exchange_delivered_order_items/ }));
    const side = screen.getByRole('complementary', { name: 'Selected step' });
    expect(within(side).getByText('exchange_delivered_order_items')).toBeInTheDocument();
    expect(await within(side).findByText(/^- "status": "delivered"$/)).toBeInTheDocument();
    expect(within(side).getByText(/^\+ "status": "exchange requested"$/)).toBeInTheDocument();
    expect(within(side).getByRole('button', { name: /Change & rerun from here/ })).toBeEnabled();
  });

  it('the final check cannot be rerun, and says why', async () => {
    page('/tests/runs/r1/b1?step=8');
    expect(await screen.findByText(/The final check can’t be rerun/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Change & rerun from here/ })).toBeDisabled();
  });

  it('the list view lists every step in order', async () => {
    page();
    await screen.findByRole('heading', { name: /Task 27/ });
    await userEvent.click(screen.getByRole('button', { name: 'List' }));
    const rows = screen.getAllByRole('row').slice(1);
    expect(rows.map((r) => r.firstChild?.textContent)).toEqual(['1', '2', '3', '4', '5', '6', '7', '8']);
  });
});
