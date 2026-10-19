import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { HostedGate } from './HostedGate';
import { afterEach, expect, it, vi } from 'vitest';
import { loadSession } from './session';
import { JoinPage } from './JoinPage';
import { AdminPage } from './AdminPage';

afterEach(() => vi.restoreAllMocks());
const reply = (status: number, body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));

it('detects local, signed-out and signed-in from /gw/api/me', async () => {
  const f = vi.spyOn(globalThis, 'fetch');
  f.mockReturnValueOnce(reply(404, {}));
  expect(await loadSession()).toEqual({ kind: 'local' });
  f.mockReturnValueOnce(reply(401, { error: { code: 'login_required' } }));
  expect(await loadSession()).toEqual({ kind: 'signed-out' });
  f.mockReturnValueOnce(reply(200, { user: { id: 'u', name: 'Ana', email: 'a@x.com', role: 'member' }, csrf: 'c'.repeat(64) }));
  expect((await loadSession()).kind).toBe('signed-in');
});

it('the join page shows who the invite is for and what is recorded, then joins with a password', async () => {
  const f = vi.spyOn(globalThis, 'fetch').mockImplementation((url) => {
    const u = String(url);
    if (u.endsWith('/gw/api/invite/tok')) return reply(200, { name: 'Ana', email: 'ana@x.com', expires_at: '2026-10-25T00:00:00Z' });
    if (u.endsWith('/gw/api/providers')) return reply(200, { password: true, google: true, github: false });
    if (u.endsWith('/gw/api/join/tok/password')) return reply(200, { ok: true });
    return reply(404, {});
  });
  const assign = vi.fn();
  render(<MemoryRouter initialEntries={['/join/tok']}><Routes><Route path="/join/:token" element={<JoinPage onSignedIn={assign} />} /></Routes></MemoryRouter>);
  expect(await screen.findByText(/Invite for Ana/)).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Continue with Google' })).toHaveAttribute('href', '/gw/api/join/tok/oidc/google');
  expect(screen.queryByRole('link', { name: 'Continue with GitHub' })).toBeNull();
  expect(screen.getByText(/We record what you do in Sculptured/)).toBeInTheDocument();
  expect(screen.getByLabelText('Your name')).toHaveValue('Ana');
  expect(screen.getByLabelText('Email')).toHaveValue('ana@x.com');
  await userEvent.clear(screen.getByLabelText('Your name'));
  await userEvent.type(screen.getByLabelText('Your name'), 'Ana');
  await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery');
  await userEvent.click(screen.getByRole('button', { name: 'Create account' }));
  await waitFor(() => expect(assign).toHaveBeenCalled());
  const call = f.mock.calls.find(([u]) => String(u).endsWith('/password'))!;
  expect(JSON.parse(String((call[1] as RequestInit).body))).toEqual({ name: 'Ana', email: 'ana@x.com', password: 'correct horse battery' });
});

it('the admin page creates an invite and shows its link once', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.endsWith('/gw/api/admin/invites') && (init?.method ?? 'GET') === 'POST') return reply(200, { id: 'i1', link: 'https://app.x/join/abc' });
    if (u.endsWith('/gw/api/admin/invites')) return reply(200, []);
    if (u.endsWith('/gw/api/admin/users')) return reply(200, []);
    return reply(404, {});
  });
  render(<MemoryRouter><AdminPage csrf={'c'.repeat(64)} /></MemoryRouter>);
  await userEvent.type(await screen.findByLabelText('Name'), 'Ana');
  await userEvent.click(screen.getByRole('button', { name: 'Create invite link' }));
  expect(await screen.findByText('https://app.x/join/abc')).toBeInTheDocument();
  expect(screen.getByText(/shown only now/)).toBeInTheDocument();
});

it('an unavailable gateway is an error, never a signed-in session without a user', async () => {
  vi.spyOn(globalThis, 'fetch').mockReturnValueOnce(reply(503, { error: { code: 'database_unavailable', message: 'busy' } }));
  await expect(loadSession()).rejects.toThrow();
});

it('signed out, Community pages open without sending you to sign in', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation((url) => (String(url).endsWith('/gw/api/me') ? reply(401, { error: { code: 'login_required' } }) : reply(404, {})));
  render(<MemoryRouter initialEntries={['/community/people']}><HostedGate><Routes><Route path="/community/*" element={<p>community page</p>} /><Route path="/login" element={<p>login page</p>} /></Routes></HostedGate></MemoryRouter>);
  expect(await screen.findByText('community page')).toBeInTheDocument();
});
