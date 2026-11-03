import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { openRigWords } from '../test-vocab';
import type { Branch, RunSummary } from '../tests/types';
import { FakeChannel } from '../workspace/testkit';
import { ArenaPanePage } from './ArenaControlPage';
import { arenaDefaults } from './defaults';
import { legacyTestsPath } from './paths';
import { ArenaSection, TestsRedirect } from './routes';

const at = (s: number) => new Date(Date.parse('2026-11-02T10:00:00Z') + s * 1000).toISOString();
const team = { id: 't', name: 't', agents: [{ id: 'triage', name: 'Triage', kind: 'claude_code' as const, model: null, prompt: '' }] };
const branch = (id: string, parent: string | null, start: number | null, t: number): Branch => ({
  id, run_id: 'r1', parent_id: parent, start_seq: start, team, changes: null, fork_note: null,
  status: 'passed', error: null, created_at: at(t), ended_at: at(t + 10),
});
const HEALTH = {
  claude: true, codex: false, open_available: false, disk: { bytes: 2 ** 30, limit_bytes: 10 * 2 ** 30 },
  benchmark: { name: 'retail-plant', version: '1.2', tasks: 3, world: [{ part: 'retail', integration: 'mongo' }] },
};
const WORLD = {
  parts: [
    { name: 'retail', integration: 'mongo', version: '7.0', answered: true, restore: { exact: 'save', approximate: false }, uses_clock: false, tools: ['get_order', 'exchange'] },
    { name: 'plant', integration: 'isaac', version: '6.2', answered: true, restore: { exact: 'replay', approximate: true }, uses_clock: true, tools: ['move'] },
    { name: 'ledger', integration: 'kv', version: '1', answered: true, restore: { exact: 'none', approximate: false }, uses_clock: false, tools: [] },
    { name: 'mail', integration: 'smtp', answered: false, error: 'connect ECONNREFUSED 127.0.0.1:7001' },
  ],
};
const USAGE = {
  integrations: [
    { integration: 'mongo', parts: ['retail'], bytes: 1.5 * 2 ** 30 },
    { integration: 'isaac', parts: ['plant'], bytes: 0, error: 'timed out after 5s' },
  ],
  total_bytes: 1.5 * 2 ** 30,
  limit_bytes: 10 * 2 ** 30,
};
const DOWN: [number, unknown] = [503, { error: { code: 'arena_unreachable', message: 'unreachable', raw: 'error sending request: connection refused' } }];

let runs: RunSummary[];
let gcReply: [number, unknown];
let down: boolean;
let calls: { method: string; url: string }[];

function answer(method: string, url: string): [number, unknown] {
  if (down) return DOWN;
  const path = url.replace('/api/arena', '');
  if (method === 'DELETE' && path === '/runs/r1') { runs = []; return [200, { dropped: 2 }]; }
  if (method === 'POST' && path === '/gc') return gcReply;
  const table: Record<string, unknown> = {
    '/health': HEALTH, '/runs': runs, '/tasks': [{ id: '27', reason: 'Swap the boots.' }], '/team/default': team, '/world': WORLD, '/usage': USAGE,
  };
  return path in table ? [200, table[path]] : [404, { error: { code: 'arena_not_found', message: path } }];
}

beforeEach(() => {
  localStorage.clear();
  FakeChannel.open.clear();
  vi.stubGlobal('BroadcastChannel', FakeChannel);
  runs = [{ id: 'r1', task_id: '27', created_at: at(0), branches: [branch('b1', null, null, 0), branch('b2', 'b1', 5, 30)] }];
  gcReply = [200, { removed: ['s-old-1', 's-old-2'], freed_bytes: 512 * 2 ** 20, errors: [] }];
  down = false;
  calls = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    calls.push({ method, url });
    const [status, body] = answer(method, url);
    return new Response(JSON.stringify(body), { status });
  }));
});
afterEach(() => vi.unstubAllGlobals());

const page = (path = '/arena') => render(
  <MemoryRouter initialEntries={[path]}>
    <Routes><Route path="/arena/*" element={<ArenaSection />} /></Routes>
  </MemoryRouter>,
);
const pane = (title: string) => screen.getByRole('region', { name: title });
const layoutTab = (name: string) => within(screen.getByRole('tablist', { name: 'Layouts' })).getByRole('tab', { name });

function Where() {
  const { pathname, search } = useLocation();
  return <output data-testid="at">{pathname + search}</output>;
}

describe('Arena control', () => {
  it('arena_overview_layout', async () => {
    const { container } = page();
    expect(screen.getByRole('heading', { name: /Arena control/ })).toBeInTheDocument();
    expect(within(screen.getByRole('tablist', { name: 'Layouts' })).getAllByRole('tab').map((t) => t.textContent)).toEqual(['Overview', 'Storage']);
    expect(layoutTab('Overview')).toHaveAttribute('aria-selected', 'true');
    for (const title of ['Runs', 'World', 'Engine']) expect(pane(title)).toBeInTheDocument();
    // Runs beside a column of World over Engine.
    const root = arenaDefaults().layouts[0].root;
    expect(root.kind === 'split' && root.dir).toBe('row');
    expect(root.kind === 'split' && root.kids.map((k) => (k.kind === 'split' ? k.dir : k.panes[0].type))).toEqual(['arena.runs', 'col']);
    expect(await within(pane('Engine')).findByText('Arena running')).toBeInTheDocument();
    expect(await within(pane('Runs')).findByRole('link', { name: 'Task 27' })).toHaveAttribute('href', '/arena/runs/r1/b1');
    expect(openRigWords(container)).toEqual([]);
  });

  it('tests_routes_redirect', () => {
    expect(legacyTestsPath('/tests')).toBe('/arena');
    expect(legacyTestsPath('/tests/team')).toBe('/arena');
    expect(legacyTestsPath('/tests/compare/a/b')).toBe('/arena/compare/a/b');
    for (const [from, to] of [['/tests/runs/r1/b2?step=4', '/arena/runs/r1/b2?step=4'], ['/tests/', '/arena']]) {
      const { unmount } = render(
        <MemoryRouter initialEntries={[from]}>
          <Routes><Route path="/tests/*" element={<TestsRedirect />} /><Route path="/arena/*" element={<Where />} /></Routes>
        </MemoryRouter>,
      );
      expect(screen.getByTestId('at')).toHaveTextContent(to);
      unmount();
    }
  });

  it('world_pane_shows_restore_words', async () => {
    page();
    const world = pane('World');
    const part = async (name: string) => within(await within(world).findByRole('listitem', { name: `Part ${name}` }));
    expect((await part('retail')).getByText('Exact from a save')).toBeInTheDocument();
    expect((await part('retail')).getByText('✓')).toBeInTheDocument();
    expect((await part('plant')).getByText('Exact by replay')).toBeInTheDocument();
    expect((await part('plant')).getByText('Approximate')).toBeInTheDocument();
    expect((await part('plant')).getByText('Uses the arena clock')).toBeInTheDocument();
    expect((await part('ledger')).getByText("Can't be restored")).toBeInTheDocument();
    const mail = await part('mail');
    expect(mail.getByText('Not answering')).toBeInTheDocument();
    expect(mail.getByText('connect ECONNREFUSED 127.0.0.1:7001')).toHaveAttribute('data-raw-evidence');
    expect(mail.getByText(/did not describe itself/)).toBeInTheDocument();
  });

  it('storage_clean_up', async () => {
    const user = userEvent.setup();
    page();
    await user.click(layoutTab('Storage'));
    const storage = pane('Saves and disk');
    expect(await within(storage).findByText('Saves use 1.5 GB of 10 GB')).toBeInTheDocument();
    expect(within(storage).getByText('timed out after 5s')).toHaveAttribute('data-raw-evidence');
    expect(pane('Fork tree')).toBeInTheDocument();
    expect(await within(pane('Fork tree')).findByText('Forked from Original at step 5')).toBeInTheDocument();

    gcReply = [409, { error: { code: 'arena_busy', message: 'busy', raw: 'Saves are being cleaned up; try again in a moment' } }];
    await user.click(within(storage).getByRole('button', { name: 'Clean up' }));
    expect(await within(storage).findByText(/Saves can't be cleaned up while a run is going/)).toBeInTheDocument();
    expect(within(storage).getByText(/Saves are being cleaned up/)).toHaveAttribute('data-raw-evidence');

    gcReply = [200, { removed: ['s-old-1', 's-old-2'], freed_bytes: 512 * 2 ** 20, errors: [{ integration: 'isaac', message: 'gc timed out' }] }];
    const usageReads = calls.filter((c) => c.url.endsWith('/usage')).length;
    await user.click(within(storage).getByRole('button', { name: 'Clean up' }));
    expect(await within(storage).findByText('Freed 512 MB. Removed 2 saves.')).toBeInTheDocument();
    expect(within(storage).getByText('gc timed out')).toHaveAttribute('data-raw-evidence');
    expect(calls.filter((c) => c.method === 'POST' && c.url.endsWith('/gc'))).toHaveLength(2);
    await waitFor(() => expect(calls.filter((c) => c.url.endsWith('/usage')).length).toBeGreaterThan(usageReads));
  });

  it('storage clean up waits for running branches, and says why', async () => {
    runs[0].branches[1].status = 'running';
    const user = userEvent.setup();
    page();
    await user.click(layoutTab('Storage'));
    const storage = pane('Saves and disk');
    await within(storage).findByText(/Saves use/);
    await waitFor(() => expect(within(storage).getByRole('button', { name: 'Clean up' })).toBeDisabled());
    expect(within(storage).getByText("Can't clean up while a run is going.")).toBeInTheDocument();
  });

  it('arena_down_is_plain', async () => {
    down = true;
    const user = userEvent.setup();
    const { container } = page();
    // The pane on show in that stack (a stack keeps its other tabs' bodies, hidden).
    const shown = (title: string) => within(pane(title).querySelector<HTMLElement>('.ws-pane.is-on')!);
    const plain = async (title: string) => {
      expect(await shown(title).findByText('Arena is not running')).toBeInTheDocument();
      expect(shown(title).getByText('cd core && cargo run -p arenad')).toBeInTheDocument();
    };
    for (const title of ['Runs', 'World', 'Engine']) await plain(title);
    await user.click(layoutTab('Storage'));
    for (const title of ['Saves and disk', 'Fork tree']) await plain(title);
    await user.click(screen.getByRole('button', { name: '+ Add pane' }));
    await user.click(screen.getByRole('menuitem', { name: /Compare/ }));
    await plain('Compare');
    // arenad's own words only as evidence.
    for (const n of within(container).getAllByText(/connection refused/)) expect(n).toHaveAttribute('data-raw-evidence');
    expect(openRigWords(container)).toEqual([]);
  });

  it('delete_run_confirms', async () => {
    const user = userEvent.setup();
    page();
    const runsPane = pane('Runs');
    await within(runsPane).findByRole('link', { name: 'Task 27' });
    await user.click(within(runsPane).getByRole('button', { name: 'Delete' }));
    const confirm = within(runsPane).getByRole('group', { name: 'Confirm delete' });
    expect(within(confirm).getByText('Delete this run, its branches and their saves?')).toBeInTheDocument();
    await user.click(within(confirm).getByRole('button', { name: 'Cancel' }));
    expect(calls.some((c) => c.method === 'DELETE')).toBe(false);

    await user.click(within(runsPane).getByRole('button', { name: 'Delete' }));
    await user.click(within(runsPane).getByRole('button', { name: 'Delete run' }));
    expect(calls.filter((c) => c.method === 'DELETE').map((c) => c.url)).toEqual(['/api/arena/runs/r1']);
    expect(await within(runsPane).findByText(/No runs yet/)).toBeInTheDocument();
  });

  it('a running run cannot be deleted, and says why', async () => {
    runs[0].branches[0].status = 'running';
    page();
    expect(await within(pane('Runs')).findByText("Can't delete while running")).toBeInTheDocument();
    expect(within(pane('Runs')).queryByRole('button', { name: 'Delete' })).toBeNull();
  });

  it('runs pane starts a run and opens its branch', async () => {
    const user = userEvent.setup();
    calls = [];
    const fetchMock = vi.mocked(fetch);
    const base = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (url, init) => (init?.method === 'POST' && String(url).endsWith('/runs')
      ? new Response(JSON.stringify({ run_id: 'r9', branch_id: 'b9' }), { status: 200 }) : base(url, init)));
    render(
      <MemoryRouter initialEntries={['/arena']}>
        <Routes><Route path="/arena" element={<ArenaSection />} /><Route path="/arena/runs/:run/:branch" element={<Where />} /></Routes>
      </MemoryRouter>,
    );
    const runsPane = pane('Runs');
    await within(runsPane).findByRole('option', { name: /27 · Swap the boots/ });
    await user.click(within(runsPane).getByRole('button', { name: 'Edit team' }));
    expect(within(runsPane).getByRole('region', { name: 'Test team' })).toBeInTheDocument();
    await user.click(within(runsPane).getByRole('button', { name: 'Start run' }));
    expect(await screen.findByTestId('at')).toHaveTextContent('/arena/runs/r9/b9');
  });

  it('a popped-out pane renders on its own route', async () => {
    const first = page();
    await within(pane('World')).findByRole('listitem', { name: 'Part retail' });
    first.unmount();
    render(
      <MemoryRouter initialEntries={['/arena/pane/ov-world']}>
        <Routes><Route path="/arena/pane/:instance" element={<ArenaPanePage />} /></Routes>
      </MemoryRouter>,
    );
    expect(screen.getByRole('heading', { name: 'World' })).toBeInTheDocument();
    expect(await screen.findByRole('listitem', { name: 'Part plant' })).toBeInTheDocument();
  });
});
