import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SUITES_MISSING } from '../control/org/panes';
import { driveWords, RunInArena } from './RunInArena';

type Answer = [number, unknown];

/** `/api/*` answered from `answers` (by method and path); every call is kept. */
function stub(answers: Record<string, Answer | (() => Answer)>) {
  const calls: { method: string; url: string; body?: unknown }[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    calls.push({ method, url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    const a = answers[`${method} ${url}`];
    const [status, body] = typeof a === 'function' ? a() : a ?? [404, { error: { code: 'not_found', message: url } }];
    return new Response(JSON.stringify(body), { status });
  }));
  return calls;
}

afterEach(() => vi.unstubAllGlobals());

const page = (blocked: string | null = null) => render(<MemoryRouter><RunInArena team="larch" blocked={blocked} /></MemoryRouter>);

describe('Run in Arena from a board', () => {
  it('starts an external run of the chosen task, lists it with a link to its Control, and says suites are not available yet', async () => {
    let runs: unknown[] = [];
    const calls = stub({
      'GET /api/arena/health': [200, { claude: true, codex: true, open_available: false }],
      'GET /api/arena/tasks': [200, [{ id: 'base', reason: 'The batch failed EM' }, { id: 'short-dated', reason: 'Stock is short-dated' }]],
      'GET /api/arena/suites': [404, { error: { code: 'arena_not_found', message: 'no route' } }],
      'GET /api/teams/larch/arena-runs': () => [200, { runs }],
      'POST /api/teams/larch/arena-runs': () => {
        runs = [{ run_id: 'r9', branch_id: 'b9', team: 'larch-arena-b9', task_id: 'short-dated', state: { state: 'driving' } }];
        return [200, { run_id: 'r9', branch_id: 'b9', team: 'larch-arena-b9' }];
      },
    });
    page();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Run in Arena…' }));
    const panel = within(await screen.findByRole('dialog', { name: 'Run in Arena' }));
    await user.selectOptions(await panel.findByRole('combobox', { name: 'Task' }), 'short-dated');
    expect(await panel.findByText(SUITES_MISSING)).toBeInTheDocument();
    expect(panel.getByRole('button', { name: 'Run a test suite' })).toBeDisabled();
    await user.click(panel.getByRole('button', { name: 'Start in Arena' }));
    expect(calls.find((c) => c.method === 'POST')?.body).toMatchObject({ task_id: 'short-dated' });
    const link = await panel.findByRole('link', { name: 'Task short-dated' });
    expect(link).toHaveAttribute('href', '/arena/runs/r9/b9');
    expect(within(panel.getByRole('list', { name: 'Arena runs of this team' })).getByText('running in Arena')).toBeInTheDocument();
  });

  it('says why it cannot start: the design has problems, or Arena is not answering', async () => {
    stub({
      'GET /api/arena/health': [200, { claude: true, codex: true, open_available: false }],
      'GET /api/arena/tasks': [200, [{ id: 'base', reason: 'r' }]],
      'GET /api/teams/larch/arena-runs': [200, { runs: [] }],
    });
    page('Fix 2 things on the design first.');
    await userEvent.click(screen.getByRole('button', { name: 'Run in Arena…' }));
    const start = await screen.findByRole('button', { name: 'Start in Arena' });
    expect(start).toBeDisabled();
    expect(start).toHaveAccessibleDescription('Fix 2 things on the design first.');
    vi.unstubAllGlobals();
    stub({ 'GET /api/arena/health': [503, { error: { code: 'arena_unreachable', message: 'down' } }], 'GET /api/teams/larch/arena-runs': [200, { runs: [] }] });
    document.body.innerHTML = '';
    page();
    await userEvent.click(screen.getByRole('button', { name: 'Run in Arena…' }));
    expect(await screen.findByRole('link', { name: 'Open Arena control' })).toHaveAttribute('href', '/arena');
  });

  it("a finished run says why it finished and Arena's outcome", () => {
    expect(driveWords({ state: 'finished', why: 'the team stopped', status: 'failed', error: null })).toEqual({ glyph: '■', text: 'finished (the team stopped); Arena: failed' });
  });
});
