import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { ComparePage } from './ComparePage';
import type { BranchDetail, CompareResponse, CompareSide, RunSummary } from './types';

const at = (s: number) => new Date(Date.parse('2026-10-30T10:00:00Z') + s * 1000).toISOString();
const team = { id: 't', name: 't', agents: [{ id: 'solo', name: 'Solo', kind: 'open' as const, model: null, prompt: '' }] };
const base: BranchDetail = {
  id: 'b1', run_id: 'r1', parent_id: null, start_seq: null, changes: null, fork_note: null, status: 'passed',
  error: null, created_at: at(0), ended_at: at(20), team, events: [], restore_mode: null, restored: [],
};
const child: BranchDetail = {
  ...base, id: 'b2', parent_id: 'b1', start_seq: 5, created_at: at(30), changes: {},
  restore_mode: 'approximate', restored: [{ part: 'retail', how: 'save' }, { part: 'plant', how: 'approximate' }],
};
const side = (b: BranchDetail): CompareSide =>
  ({ status: 'passed', seconds: 20, tokens: 100, cost_usd: null, cost_source: null, restore_mode: b.restore_mode, restored: b.restored });
const runs: RunSummary[] = [{ id: 'r1', task_id: 't1', created_at: at(0), branches: [base, child] }];
let cmp: CompareResponse;

function answer(url: string): unknown {
  if (url.includes('/compare?')) return cmp;
  if (url.endsWith('/branches/b1')) return base;
  if (url.endsWith('/branches/b2')) return child;
  if (url.endsWith('/runs')) return runs;
  throw new Error(`unexpected ${url}`);
}

beforeEach(() => {
  cmp = { a: side(base), b: side(child), first_divergent_seq: null, differing_rows: [], not_compared: ['counter'] };
  vi.stubGlobal('fetch', vi.fn(async (url: string) => new Response(JSON.stringify(answer(url)), { status: 200 })));
});
afterEach(() => vi.unstubAllGlobals());

const page = () => render(
  <MemoryRouter initialEntries={['/tests/compare/b1/b2']}>
    <Routes><Route path="/tests/compare/:a/:b" element={<ComparePage />} /></Routes>
  </MemoryRouter>,
);

describe('ComparePage', () => {
  it('an approximate side is labelled, with the caveat about its differences', async () => {
    page();
    const sides = await screen.findAllByRole('link', { name: /Rerun|first run/ });
    expect(within(sides[1]).getByText('Approximate restore')).toBeInTheDocument();
    expect(within(sides[0]).queryByText('Approximate restore')).toBeNull();
    expect(screen.getByText('Differences may come from the approximate restore as well as the change.')).toBeInTheDocument();
  });

  it('a part not compared never reads as identical', async () => {
    page();
    const rows = await screen.findByRole('region', { name: 'Database differences' });
    expect(within(rows).getByText('Not compared: counter (it keeps no saves)')).toBeInTheDocument();
    expect(within(rows).getByRole('heading')).toHaveTextContent('Final database: no differences in the parts compared');
  });
});
