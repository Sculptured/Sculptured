import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { OverviewTeam } from '../../generated/OverviewTeam';
import { openRigWords } from '../../test-vocab';
import { FakeEventSource } from '../../test-fixtures';
import { FakeChannel } from '../../workspace/testkit';
import { branchB1 } from '../simHarness';
import { OrgControlPage } from './OrgControlPage';
import { SUITES_MISSING } from './panes';
import { byAttention, recentRuns, unitAlarms, unitState } from './units';

const team = (over: Partial<OverviewTeam> = {}): OverviewTeam => ({
  id: 't1', name: 'Larchmont team', started: true, desired: 'running', agents: 3, running: 3, attention: 0, failed: 0, open_questions: 0,
  actions_per_min: [...new Array<number>(14).fill(0), 4], failed_actions: 0, last_action: { at_ms: Date.parse('2026-11-05T10:00:00Z'), seat: 'team.a@t1', tool: 'mcp__erp__get_lot' },
  run_tokens: 12_000, run_cost_usd: 0.42, run_cost_complete: true, ...over,
});

describe('organisation units', () => {
  it('a unit says its state in a glyph and a word, and counts its alarms', () => {
    expect(unitState(team())).toEqual({ glyph: '⣟', word: 'running', tone: 'ok' });
    expect(unitState(team({ failed: 1 }))).toMatchObject({ glyph: '✕', word: '1 failed', tone: 'bad' });
    expect(unitState(team({ started: false, desired: null }))).toMatchObject({ word: 'not started' });
    expect(unitState(team({ running: 0, desired: 'stopped' }))).toMatchObject({ glyph: '■', word: 'stopped' });
    expect(unitAlarms(team({ attention: 1, open_questions: 2, failed_actions: 1 }))).toEqual({ count: 4, words: ['1 needs attention', '2 questions open', '1 call failed (15 min)'] });
    const sorted = [team({ id: 'b', name: 'B', running: 0 }), team({ id: 'a', name: 'A' }), team({ id: 'c', name: 'C', failed: 1 })].sort(byAttention);
    expect(sorted.map((t) => t.id)).toEqual(['c', 'a', 'b']);
  });

  it('recent Arena runs, newest first, each by its first branch', () => {
    const b = branchB1();
    const runs = [{ id: 'r1', task_id: '27', created_at: '2026-11-05T09:00:00Z', branches: [b] }, { id: 'r2', task_id: '28', created_at: '2026-11-05T10:00:00Z', branches: [{ ...b, id: 'b9', team: { ...b.team, name: 'Coordinated team' } }] }];
    expect(recentRuns(runs).map((r) => [r.run, r.branch, r.team, r.status])).toEqual([['r2', 'b9', 'Coordinated team', 'passed'], ['r1', 'b1', 't', 'passed']]);
  });
});

function stubFetch(answers: Record<string, [number, unknown]>) {
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    const [status, body] = answers[url] ?? [404, { error: { code: 'not_found', message: url } }];
    return new Response(JSON.stringify(body), { status });
  }));
}

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal('BroadcastChannel', FakeChannel);
  vi.stubGlobal('EventSource', FakeEventSource);
});
afterEach(() => vi.unstubAllGlobals());

describe('Control room', () => {
  it('lists suites using their report figures and links to the configured Arena page', async () => {
    stubFetch({
      '/api/arena/suites': [200, [{ id: 's1', name: 'Safety suite', team: { name: 'Team A' }, tasks: ['a', 'b'], repeats: 2, status: 'running', runs: 4, ended: 2, passed: 1 }]],
      '/api/arena/suites/s1': [200, { summary: { runs: 4, ended: 3, pass_rate: 0.5, safety_violations: 1 } }],
    });
    render(<MemoryRouter><OrgControlPage /></MemoryRouter>);
    const table = await screen.findByRole('table', { name: 'Test suites' });
    expect(within(table).getByRole('link', { name: 'Safety suite' })).toHaveAttribute('href', '/api/arena/suite-page/s1');
    expect(within(table).getAllByRole('row')[1]).toHaveTextContent('Team A⣟ running3 / 450%1');
  });

  it('keeps listed suites visible with unknown scored figures when their report is unavailable', async () => {
    stubFetch({ '/api/arena/suites': [200, [{ id: 's1', name: 'Pending suite', team: { name: 'Team A' }, tasks: ['a'], repeats: 1, status: 'stopped', runs: 1, ended: 0, passed: 0 }]] });
    render(<MemoryRouter><OrgControlPage /></MemoryRouter>);
    const table = await screen.findByRole('table', { name: 'Test suites' });
    expect(within(table).getAllByText('unknown')).toHaveLength(2);
    expect(within(table).getByText('0 / 1')).toBeInTheDocument();
    expect(screen.queryByText(`○ ${SUITES_MISSING}`)).not.toBeInTheDocument();
  });

  it('shows an empty suite list', async () => {
    stubFetch({ '/api/arena/suites': [200, []] });
    render(<MemoryRouter><OrgControlPage /></MemoryRouter>);
    expect(await screen.findByText('No test suites yet.')).toBeInTheDocument();
  });

  it('shows every deployment as a unit with its alarms and figures, Arena runs, and says suites are not available yet', async () => {
    stubFetch({
      '/api/control/overview': [200, { at_ms: Date.parse('2026-11-05T10:01:00Z'), teams: [team(), team({ id: 't2', name: 'Single agent', failed: 1, run_cost_complete: false })] }],
      '/api/arena/runs': [200, [{ id: 'r1', task_id: '27', created_at: '2026-11-05T09:00:00Z', branches: [branchB1()] }]],
      '/api/arena/suites': [404, { error: { code: 'arena_not_found', message: 'no route' } }],
    });
    const { container } = render(<MemoryRouter><OrgControlPage /></MemoryRouter>);
    const units = await screen.findByRole('table', { name: 'Deployments' });
    const rows = within(units).getAllByRole('row').slice(1);
    expect(rows.map((r) => within(r).getAllByRole('cell')[0]!.textContent)).toEqual(['Single agent', 'Larchmont team']);
    expect(rows[0]).toHaveTextContent('✕ 1 failed');
    expect(rows[0]).toHaveTextContent('▲ 1 · 1 agent failed');
    expect(rows[0]).toHaveTextContent('≥ $0.42');
    expect(within(rows[1]!).getByRole('link', { name: 'Larchmont team' })).toHaveAttribute('href', '/p/t1/control');
    expect(rows[1]).toHaveTextContent('4/min');
    expect(rows[1]).toHaveTextContent('mcp__erp__get_lot');
    expect(await screen.findByRole('link', { name: 'Task 27' })).toHaveAttribute('href', '/arena/runs/r1/b1');
    expect(await screen.findByText(`○ ${SUITES_MISSING}`)).toBeInTheDocument();
    expect(within(screen.getByRole('list', { name: 'Organisation status' })).getByText('2 running of 2')).toBeInTheDocument();
    expect(openRigWords(container)).toEqual([]);
  });
});
