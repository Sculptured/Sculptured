import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { ConnectPage } from './ConnectPage';
import { ComputersPage } from './ComputersPage';

afterEach(() => vi.restoreAllMocks());
const reply = (status: number, body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status }));

it('approving a code names the computer first, then confirms', async () => {
  const f = vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.includes('/gw/api/device/code/WXYZ-2345')) return reply(200, { name: "Alex's laptop", platform: 'linux', user_code: 'WXYZ-2345' });
    if (u.endsWith('/gw/api/device/approve') && init?.method === 'POST') return reply(200, { ok: true });
    return reply(404, {});
  });
  render(<MemoryRouter initialEntries={['/connect?code=WXYZ-2345']}><ConnectPage /></MemoryRouter>);
  expect(await screen.findByText(/Allow ‘Alex's laptop’ \(linux\) to run agents for you\?/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Allow' }));
  expect(await screen.findByText(/Connected/)).toBeInTheDocument();
  expect(f.mock.calls.some(([u, i]) => String(u).endsWith('/approve') && JSON.parse(String((i as RequestInit).body)).user_code === 'WXYZ-2345')).toBe(true);
});

it('lists computers with their state and disconnects one', async () => {
  let revoked = false;
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.endsWith('/gw/api/devices')) return reply(200, revoked ? [] : [{ id: 'd1', name: 'Laptop', platform: 'macos', created_at: '2026-10-11T00:00:00Z', last_seen: null, connected: true }]);
    if (u.endsWith('/gw/api/devices/d1/revoke') && init?.method === 'POST') { revoked = true; return reply(200, { ok: true }); }
    return reply(404, {});
  });
  render(<MemoryRouter><ComputersPage /></MemoryRouter>);
  expect(await screen.findByText('Laptop')).toBeInTheDocument();
  expect(screen.getByText('Connected')).toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Connect a computer' })).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Back to Sculptured' })).toHaveAttribute('href', '/');
  await userEvent.click(screen.getByRole('button', { name: 'Disconnect Laptop' }));
  await waitFor(() => expect(screen.queryByText('Laptop')).toBeNull());
});

it('warns not to approve a code someone else sent, and says when and where it was requested', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation((url) => String(url).includes('/gw/api/device/code/') ? reply(200, { name: 'MacBook', platform: 'macos', user_code: 'WXYZ-2345', requested_at: '2026-10-12T01:00:00Z', requested_from: '203.0.113.9' }) : reply(404, {}));
  render(<MemoryRouter initialEntries={['/connect?code=WXYZ-2345']}><ConnectPage /></MemoryRouter>);
  expect(await screen.findByText(/Only approve a code you just saw on your own computer/)).toBeInTheDocument();
  expect(screen.getByText(/203\.0\.113\.9/)).toBeInTheDocument();
});

it('shows what each connected computer allows and its CLIs, with sign-in on that computer', async () => {
  const calls: string[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url); calls.push(`${init?.method ?? 'GET'} ${u}`);
    if (u.endsWith('/gw/api/devices')) return reply(200, [{ id: 'd1', name: 'Laptop', platform: 'linux', created_at: '2026-10-11T00:00:00Z', last_seen: null, connected: true }]);
    if (u.endsWith('/api/hosts')) return reply(200, [{ id: 'd1', drivers: ['stub'], policy: { allowed_folders: ['/home/a/proj'], access: 'full', capabilities: ['git', 'copies', 'checks', 'cli_sign_in', 'plan_usage'], platform: 'linux' } }]);
    if (u.includes('/api/clis?host=d1')) return reply(200, [{ name: 'claude', installed: true, version: '2.1', signed_in: true, account: 'a@x', detail: '' }, { name: 'codex', installed: true, version: '0.9', signed_in: false, account: null, detail: '' }]);
    if (u.includes('/api/clis/codex/login?host=d1')) return reply(200, { url: 'https://auth.example/x', output: '' });
    return reply(404, {});
  });
  render(<MemoryRouter><ComputersPage /></MemoryRouter>);
  expect(await screen.findByText('/home/a/proj')).toBeInTheDocument();
  expect(screen.getByText(/Full access/)).toBeInTheDocument();
  expect(await screen.findByText(/Claude: signed in/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Sign in to Codex on Laptop' }));
  expect(await screen.findByRole('link', { name: /Open the sign-in page/ })).toHaveAttribute('href', 'https://auth.example/x');
  expect(calls.some((c) => c.startsWith('POST') && c.includes('/api/clis/codex/login?host=d1'))).toBe(true);
});

it('sets how many agents work at once on a computer, across its teams', async () => {
  const puts: unknown[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.endsWith('/gw/api/devices')) return reply(200, [{ id: 'd1', name: 'Laptop', platform: 'linux', created_at: '2026-10-11T00:00:00Z', last_seen: null, connected: true }]);
    if (u.endsWith('/api/hosts')) return reply(200, [{ id: 'd1', drivers: ['stub'], max_working: null, policy: { allowed_folders: ['/w'], access: 'full', capabilities: [], platform: 'linux' } }]);
    if (u.endsWith('/api/hosts/d1/limit') && init?.method === 'PUT') { puts.push(JSON.parse(String(init.body))); return reply(200, { id: 'd1', max_working: 3 }); }
    if (u.includes('/api/clis?host=d1')) return reply(200, []);
    return reply(404, {});
  });
  render(<MemoryRouter><ComputersPage /></MemoryRouter>);
  const box = await screen.findByRole('spinbutton', { name: 'Most agents working at once on Laptop' });
  await userEvent.type(box, '3');
  await userEvent.click(screen.getByRole('button', { name: 'Save the cap for Laptop' }));
  await waitFor(() => expect(puts).toEqual([{ max_working: 3 }]));
  expect(await screen.findByText(/Saved/)).toBeInTheDocument();
});

it('a Community sign-in says what it can and cannot do, and Don’t allow turns it down', async () => {
  const f = vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.includes('/gw/api/device/code/WXYZ-2345')) return reply(200, { name: 'Laptop', platform: 'linux', user_code: 'WXYZ-2345', scope: 'community', label: 'Community access from ‘Laptop’' });
    if (u.endsWith('/gw/api/device/deny') && init?.method === 'POST') return reply(200, { ok: true });
    return reply(404, {});
  });
  render(<MemoryRouter initialEntries={['/connect?code=WXYZ-2345']}><ConnectPage /></MemoryRouter>);
  expect(await screen.findByText('Allow Community access from ‘Laptop’?')).toBeInTheDocument();
  expect(screen.getByText(/It cannot run agents/)).toBeInTheDocument();
  expect(screen.queryByText(/to run agents for you/)).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Don’t allow' }));
  expect(await screen.findByText(/Turned down/)).toBeInTheDocument();
  expect(f.mock.calls.some(([u, i]) => String(u).endsWith('/gw/api/device/deny') && JSON.parse(String((i as RequestInit).body)).user_code === 'WXYZ-2345')).toBe(true);
});

it('lists Community access by its label', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation((url) => String(url).endsWith('/gw/api/devices')
    ? reply(200, [{ id: 'd2', name: 'Laptop', platform: 'linux', created_at: '2026-10-11T00:00:00Z', last_seen: null, connected: false, scope: 'community', label: 'Community access from ‘Laptop’' }])
    : reply(404, {}));
  render(<MemoryRouter><ComputersPage /></MemoryRouter>);
  expect(await screen.findByText('Community access from ‘Laptop’')).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Disconnect Community access from ‘Laptop’' })).toBeInTheDocument();
});
