import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { SetupGate } from './SetupGate';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));
const SIGNED_IN: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x.com', role: 'member' }, csrf: 'c' };

type World = { progress?: { done: string[]; skipped: string[] }; hosts?: { id: string }[]; signedIn?: boolean; connections?: unknown[] };

type Req = { u: string; method: string; init?: RequestInit };
type Route = (w: World, q: Req, puts: { done: string[]; skipped: string[] }[]) => Promise<Response> | null;

const ROUTES: Route[] = [
  (_w, { u, method, init }, puts) => { if (!u.endsWith('/api/setup') || method !== 'PUT') return null; const b = JSON.parse(String(init!.body)); puts.push(b); return reply(b); },
  (w, { u }) => (u.endsWith('/api/setup') ? reply(w.progress ?? { done: [], skipped: [] }) : null),
  (w, { u }) => (u.endsWith('/api/hosts') ? reply((w.hosts ?? []).map((h) => ({ ...h, drivers: ['claude'], policy: null }))) : null),
  (w, { u }) => (u.includes('/api/clis') ? reply([{ name: 'claude', installed: true, version: '2', signed_in: w.signedIn ?? false, account: null, detail: '' }]) : null),
  (w, { u }) => (u.endsWith('/gw/api/connections') ? reply({ connections: w.connections ?? [] }) : null),
];

/** A fake core and gateway; returns the calls made and the progress saved. */
function serve(w: World) {
  const calls: string[] = [];
  const puts: { done: string[]; skipped: string[] }[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const q = { u: String(url), method: init?.method ?? 'GET', init };
    calls.push(`${q.method} ${q.u}`);
    for (const route of ROUTES) { const r = route(w, q, puts); if (r) return r; }
    return reply({}, 404);
  });
  return { calls, puts };
}

function at(path: string, session: HostedSession = SIGNED_IN) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <HostedProvider value={session}>
        <SetupGate>
          <Routes>
            <Route path="/" element={<p>projects page</p>} />
            <Route path="/p/:team/board" element={<p>board page</p>} />
            <Route path="/setup" element={<p>setup page</p>} />
            <Route path="/account" element={<p>account page</p>} />
            <Route path="/community/*" element={<p>community page</p>} />
          </Routes>
        </SetupGate>
      </HostedProvider>
    </MemoryRouter>,
  );
}

it('sends a new hosted user from the app to /setup', async () => {
  serve({});
  at('/p/t1/board');
  expect(await screen.findByText('setup page')).toBeInTheDocument();
});

it('keeps Account and Community open while setup is not done', async () => {
  serve({});
  at('/account');
  expect(await screen.findByText('account page')).toBeInTheDocument();
  at('/community/people');
  expect(await screen.findByText('community page')).toBeInTheDocument();
});

it('never redirects an existing user whose computer and sign-in are already there, and marks those steps done', async () => {
  const { puts } = serve({ hosts: [{ id: 'd1' }], signedIn: true });
  at('/');
  expect(await screen.findByText('projects page')).toBeInTheDocument();
  await waitFor(() => expect(puts.at(-1)?.done).toEqual(expect.arrayContaining(['welcome', 'computer', 'models'])));
  expect(screen.queryByText('setup page')).toBeNull();
});

it('a working model connection counts as models done', async () => {
  serve({ hosts: [{ id: 'd1' }], connections: [{ id: 'c1', name: 'Key', provider: 'anthropic-key', model: null, key_last4: '1234', checked: { ok: true, at: '2026-10-21T00:00:00Z', detail: '' }, created_at: '2026-10-21T00:00:00Z' }] });
  at('/');
  expect(await screen.findByText('projects page')).toBeInTheDocument();
});

it('saved progress opens the app without reading computers', async () => {
  const { calls } = serve({ progress: { done: ['welcome', 'computer', 'models'], skipped: [] } });
  at('/');
  expect(await screen.findByText('projects page')).toBeInTheDocument();
  expect(calls.some((c) => c.endsWith('/api/hosts'))).toBe(false);
});

it('has no gate on your own computer', async () => {
  serve({});
  at('/', { kind: 'local' });
  expect(await screen.findByText('projects page')).toBeInTheDocument();
});
