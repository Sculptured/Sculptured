import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { SetupGate, SETUP_POLL } from './SetupGate';
import { SetupPage } from './SetupPage';
import { openRigWords } from '../test-vocab';

const SIGNED_IN: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x.com', role: 'member' }, csrf: 'c' };
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));
const SECRET = 'sk-ant-api03-very-secret-9876';

type Conn = { id: string; name: string; provider: string; model: string | null; key_last4: string; checked: { ok: boolean; at: string; detail: string } | null; created_at: string };
type World = { done?: string[]; skipped?: string[]; hosts: { id: string }[]; signedIn?: boolean; connections: Conn[]; check?: { ok: boolean; detail: string } };

beforeEach(() => { SETUP_POLL.ms = 30; });
afterEach(() => vi.restoreAllMocks());

function serve(w: World) {
  const posts: { url: string; body: unknown }[] = [];
  const puts: { done: string[]; skipped: string[] }[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    const method = init?.method ?? 'GET';
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    if (u.endsWith('/api/setup') && method === 'PUT') { puts.push(body); return reply(body); }
    if (u.endsWith('/api/setup')) return reply({ done: w.done ?? ['welcome'], skipped: w.skipped ?? [] });
    if (u.endsWith('/api/hosts')) return reply(w.hosts.map((h) => ({ ...h, drivers: ['claude'], policy: { allowed_folders: [], access: 'full', capabilities: ['cli_sign_in'], platform: 'linux' } })));
    if (u.includes('/api/clis')) return reply([{ name: 'claude', installed: true, version: '2', signed_in: w.signedIn ?? false, account: null, detail: '' }, { name: 'codex', installed: true, version: '1', signed_in: false, account: null, detail: '' }]);
    if (u.endsWith('/gw/api/devices')) return reply(w.hosts.map((h) => ({ id: h.id, name: 'Laptop', platform: 'linux', created_at: '', last_seen: null, connected: true })));
    if (u.endsWith('/gw/api/releases')) return reply({ targets: ['x86_64-unknown-linux-gnu'] });
    if (u.endsWith('/gw/api/clusters')) return reply({ clusters: [], available: true });
    if (u.endsWith('/gw/api/connections') && method === 'POST') {
      posts.push({ url: u, body });
      const c: Conn = { id: `c${w.connections.length + 1}`, name: body.name, provider: body.provider, model: body.model ?? null, key_last4: String(body.key).slice(-4), checked: null, created_at: '2026-10-21T00:00:00Z' };
      w.connections = [...w.connections, c];
      return reply(c);
    }
    if (u.endsWith('/gw/api/connections')) return reply({ connections: w.connections });
    const check = u.match(/\/gw\/api\/connections\/([^/]+)\/check$/);
    if (check && method === 'POST') {
      posts.push({ url: u, body });
      const result = w.check ?? { ok: true, detail: 'Key works' };
      w.connections = w.connections.map((c) => (c.id === check[1] ? { ...c, checked: { ...result, at: '2026-10-21T00:00:00Z' } } : c));
      // The gateway answers with the connection, its `checked` filled in.
      return reply(w.connections.find((c) => c.id === check[1]));
    }
    return reply({}, 404);
  });
  return { posts, puts };
}

function open(path = '/setup') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <HostedProvider value={SIGNED_IN}>
        <SetupGate>
          <Routes>
            <Route path="/setup" element={<SetupPage />} />
            <Route path="/" element={<p>projects page</p>} />
          </Routes>
        </SetupGate>
      </HostedProvider>
    </MemoryRouter>,
  );
}

it('resumes at the first step left, with the steps in order and their state in words', async () => {
  serve({ hosts: [], connections: [] });
  open();
  expect(await screen.findByRole('heading', { level: 2, name: 'Connect a computer' })).toBeInTheDocument();
  const bar = screen.getByRole('navigation', { name: 'Setup steps' });
  const steps = within(bar).getAllByRole('listitem');
  expect(steps.map((s) => s.textContent)).toEqual([
    expect.stringMatching(/Welcome.*Done/),
    expect.stringMatching(/Computer/),
    expect.stringMatching(/Models/),
    expect.stringMatching(/More computers.*Optional/),
    expect.stringMatching(/First project.*Optional/),
  ]);
  expect(within(steps[1]).getByRole('button')).toHaveAttribute('aria-current', 'step');
  expect(openRigWords(document.body)).toEqual([]);
});

it('the Computer step completes live when a computer connects', async () => {
  const w: World = { hosts: [], connections: [] };
  const { puts } = serve(w);
  open();
  const next = await screen.findByRole('button', { name: 'Continue' });
  expect(next).toBeDisabled();
  expect(screen.getByText(/Waiting for a computer to connect/)).toBeInTheDocument();
  w.hosts = [{ id: 'd1' }];
  expect(await screen.findByText(/Laptop is connected/)).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled());
  await userEvent.click(screen.getByRole('button', { name: 'Continue' }));
  await waitFor(() => expect(puts.at(-1)?.done).toEqual(expect.arrayContaining(['welcome', 'computer'])));
  expect(await screen.findByRole('heading', { level: 2, name: 'Connect a model' })).toBeInTheDocument();
});

it('a cluster can be the computer', async () => {
  serve({ hosts: [], connections: [] });
  open();
  await userEvent.click(await screen.findByRole('button', { name: 'My computer is a cluster' }));
  expect(await screen.findByRole('region', { name: 'Clusters' })).toBeInTheDocument();
});

it('adding an Anthropic key sends it once and only ever shows its last four characters', async () => {
  const { posts } = serve({ done: ['welcome', 'computer'], hosts: [{ id: 'd1' }], connections: [] });
  open();
  const form = await screen.findByRole('form', { name: 'Claude — API key' });
  await userEvent.type(within(form).getByLabelText('Name'), 'Work key');
  const key = within(form).getByLabelText('Anthropic API key');
  expect(key).toHaveAttribute('type', 'password');
  await userEvent.type(key, SECRET);
  await userEvent.click(within(form).getByRole('button', { name: 'Save and check' }));
  await waitFor(() => expect(posts[0]?.body).toEqual({ name: 'Work key', provider: 'anthropic-key', key: SECRET }));
  expect(await screen.findByText('••••9876')).toBeInTheDocument();
  await waitFor(() => expect(posts.some((p) => p.url.endsWith('/connections/c1/check'))).toBe(true));
  expect(document.body.innerHTML).not.toContain(SECRET);
  expect(key).toHaveValue('');
});

it('offers an Anthropic key form and no OpenRouter one (not finished yet)', async () => {
  serve({ done: ['welcome', 'computer'], hosts: [{ id: 'd1' }], connections: [] });
  open();
  expect(await screen.findByRole('form', { name: 'Claude — API key' })).toBeInTheDocument();
  expect(screen.queryByRole('form', { name: 'Claude via OpenRouter' })).not.toBeInTheDocument();
  expect(screen.queryByText(/OpenRouter/)).not.toBeInTheDocument();
});

const unchecked: Conn = { id: 'c1', name: 'Work key', provider: 'anthropic-key', model: null, key_last4: '9876', checked: null, created_at: '2026-10-21T00:00:00Z' };

it('Check shows what the provider answered', async () => {
  serve({ done: ['welcome', 'computer'], hosts: [{ id: 'd1' }], connections: [unchecked], check: { ok: false, detail: 'invalid x-api-key' } });
  open();
  const row = await screen.findByRole('listitem', { name: 'Work key' });
  expect(row).toHaveTextContent('Not checked yet');
  await userEvent.click(within(row).getByRole('button', { name: 'Check Work key' }));
  expect(await within(row).findByText(/Check failed/)).toBeInTheDocument();
  expect(within(row).getByText('invalid x-api-key')).toHaveAttribute('data-raw-evidence');
  expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
});

it('Models is done once a check passes', async () => {
  const { puts } = serve({ done: ['welcome', 'computer'], hosts: [{ id: 'd1' }], connections: [unchecked] });
  open();
  const row = await screen.findByRole('listitem', { name: 'Work key' });
  expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
  await userEvent.click(within(row).getByRole('button', { name: 'Check Work key' }));
  expect(await within(row).findByText(/Works/)).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled());
  await userEvent.click(screen.getByRole('button', { name: 'Continue' }));
  await waitFor(() => expect(puts.at(-1)?.done).toContain('models'));
});

it('shows each computer’s Claude Code and Codex sign-in on the Models step', async () => {
  serve({ done: ['welcome', 'computer'], hosts: [{ id: 'd1' }], connections: [] });
  open();
  expect(await screen.findByRole('button', { name: 'Sign in to Claude on Laptop' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Sign in to Codex on Laptop' })).toBeInTheDocument();
});

it('optional steps can be skipped, and say so', async () => {
  const { puts } = serve({ done: ['welcome', 'computer', 'models'], hosts: [{ id: 'd1' }], connections: [], signedIn: true });
  open();
  expect(await screen.findByText(/coordinate by messages/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Skip' }));
  await waitFor(() => expect(puts.at(-1)?.skipped).toContain('computers'));
  expect(await screen.findByRole('heading', { level: 2, name: 'Your first project' })).toBeInTheDocument();
});
