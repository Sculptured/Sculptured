import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import type { AccountUsage } from '../generated/AccountUsage';
import type { HostedSession } from '../hosted/session';

let session: HostedSession = { kind: 'local' };
vi.mock('../hosted/HostedGate', () => ({ useHosted: () => session }));
const { AccountPage } = await import('./AccountPage');

afterEach(() => { vi.restoreAllMocks(); session = { kind: 'local' }; });
const reply = (status: number, body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));

const days = Array.from({ length: 30 }, (_, i) => ({ day: new Date(Date.UTC(2026, 8, 14 + i)).toISOString().slice(0, 10), tokens: 0 }));
days[29] = { day: '2026-10-13', tokens: 1515 };
days[20] = { day: '2026-10-04', tokens: 40 };
const usage: AccountUsage = {
  totals: { input: 420, output: 45, cache_read: 1000, cache_write: 50, reports: 3 },
  cost: { usd: 1.25, by_source: [{ source: 'claude-code', usd: 0.25 }, { source: 'openai-list-prices', usd: 1 }], reports_without_cost: 1, openai_prices_checked: '2026-10-12' },
  by_day: days,
  by_runtime: [{ runtime: 'claude-code', tokens: 1185, usd: 0.25 }, { runtime: 'codex', tokens: 330, usd: 1 }],
  top_teams: [{ team: 'shop', name: 'Shop rebuild', tokens: 1185, usd: 0.25 }, { team: 'docs', name: 'Docs pass', tokens: 330, usd: 1 }],
  activity: { teams: 4, turns_completed: 12, agent_sessions: 6, messages_you_sent: 9 },
};

function serve(body: AccountUsage = usage) {
  const calls: string[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    calls.push(`${init?.method ?? 'GET'} ${u}`);
    if (u.endsWith('/api/account/usage')) return reply(200, body);
    return reply(200, { ok: true });
  });
  render(<MemoryRouter><AccountPage /></MemoryRouter>);
  return calls;
}

it('shows who is signed in and signs out from the account page', async () => {
  session = { kind: 'signed-in', user: { id: 'u1', name: 'Ana Lee', email: 'ana@x.com', role: 'member' }, csrf: 'c'.repeat(64) };
  const assign = vi.fn();
  vi.spyOn(window, 'location', 'get').mockReturnValue({ ...window.location, assign } as Location);
  const calls = serve();
  expect(await screen.findByText('Ana Lee')).toBeInTheDocument();
  expect(screen.getByText('ana@x.com')).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
  expect(calls).toContain('POST /gw/api/logout');
  await vi.waitFor(() => expect(assign).toHaveBeenCalledWith('/login'));
});

it('on your own computer there is no sign-in, only your usage', async () => {
  serve();
  expect(await screen.findByText(/runs on this computer/)).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Sign out' })).toBeNull();
});

it('sums your tokens and costs, saying where each cost comes from', async () => {
  serve();
  const tokens = await screen.findByRole('group', { name: 'Tokens used' });
  expect(tokens).toHaveTextContent('1.5k');
  expect(tokens).toHaveTextContent('420 input · 45 output · 1.0k cache read · 50 cache write');
  const cost = screen.getByRole('group', { name: 'Cost' });
  expect(cost).toHaveTextContent('$1.25');
  expect(cost).toHaveTextContent('$0.25 reported by Claude Code');
  expect(cost).toHaveTextContent('$1.00 at OpenAI list prices (checked 2026-10-12)');
  expect(cost).toHaveTextContent('1 report had no cost');
  expect(screen.getByRole('group', { name: 'Turns completed' })).toHaveTextContent('12');
  expect(screen.getByRole('group', { name: 'Messages you sent' })).toHaveTextContent('9');
});

it('charts the last 30 days, names the busiest one, and offers the numbers as a table', async () => {
  serve();
  const chart = await screen.findByRole('img', { name: /Tokens per day, last 30 days/ });
  expect(chart.querySelectorAll('rect[data-day]')).toHaveLength(30);
  expect(screen.getByText(/Busiest day: Oct 13/)).toHaveTextContent('1.5k tokens');
  await userEvent.click(screen.getByText('Show as a table'));
  const rows = within(screen.getByRole('table', { name: 'Tokens per day' })).getAllByRole('row');
  expect(rows).toHaveLength(31);
});

it('splits use by CLI and lists the teams that used the most, linking to them', async () => {
  serve();
  const clis = await screen.findByRole('list', { name: 'By CLI' });
  expect(within(clis).getByText('Claude Code').closest('li')).toHaveTextContent('1.2k tokens');
  expect(within(clis).getByText('Codex').closest('li')).toHaveTextContent('$1.00');
  const teams = screen.getByRole('list', { name: 'Teams that used the most' });
  expect(within(teams).getByRole('link', { name: 'Shop rebuild' })).toHaveAttribute('href', '/team/shop/structure');
});

it('never shows $0.00 when no report had a cost', async () => {
  serve({ ...usage, cost: { ...usage.cost, usd: 0, by_source: [], reports_without_cost: 3 } });
  const cost = await screen.findByRole('group', { name: 'Cost' });
  expect(cost).toHaveTextContent('Unknown');
  expect(cost).not.toHaveTextContent('$0.00');
  expect(cost).toHaveTextContent('3 reports had no cost');
});

it('says plainly when nothing has run yet', async () => {
  serve({ ...usage, totals: { input: 0, output: 0, cache_read: 0, cache_write: 0, reports: 0 }, by_day: days.map((d) => ({ ...d, tokens: 0 })), by_runtime: [], top_teams: [], cost: { ...usage.cost, usd: 0, by_source: [], reports_without_cost: 0 } });
  expect(await screen.findByText(/No agent has used any tokens yet/)).toBeInTheDocument();
});

it('local: shows the Community card and turns Community off', async () => {
  const { resetCommunityLink } = await import('../community/link');
  resetCommunityLink();
  const calls: string[] = [];
  let on = true;
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    const m = init?.method ?? 'GET';
    calls.push(`${m} ${u}`);
    if (u.endsWith('/api/account/usage')) return reply(200, usage);
    if (u.endsWith('/api/community-link') && m === 'GET') return reply(200, on ? { enabled: true, origin: 'https://app.usesculptured.com', signed_in: true, account: { name: 'You Example', handle: 'you-example' } } : { enabled: false, origin: null, signed_in: false, account: null });
    if (u.endsWith('/api/community-link') && m === 'DELETE') { on = false; return reply(200, { enabled: false }); }
    return reply(200, {});
  });
  render(<MemoryRouter><AccountPage /></MemoryRouter>);
  const card = await screen.findByRole('region', { name: 'Community' });
  expect(await within(card).findByText('Signed in as You Example (@you-example)')).toBeInTheDocument();
  expect(within(card).getByText(/app\.usesculptured\.com/)).toBeInTheDocument();
  await userEvent.click(within(card).getByRole('button', { name: 'Turn off Community' }));
  expect(calls).toContain('DELETE /api/community-link');
  expect(await within(card).findByRole('link', { name: 'Open Community' })).toHaveAttribute('href', '/community');
  resetCommunityLink();
});
