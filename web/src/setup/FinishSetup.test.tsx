import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { FinishSetup } from './FinishSetup';
import { SetupGate } from './SetupGate';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));
const SIGNED_IN: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x.com', role: 'member' }, csrf: 'c' };

function serve(progress: { done: string[]; skipped: string[] }) {
  const puts: { done: string[]; skipped: string[] }[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.endsWith('/api/setup') && init?.method === 'PUT') { const b = JSON.parse(String(init.body)); puts.push(b); return reply(b); }
    if (u.endsWith('/api/setup')) return reply(progress);
    if (u.includes('/api/clis')) return reply([{ name: 'claude', installed: true, version: '2', signed_in: true, account: null, detail: '' }]);
    return reply({}, 404);
  });
  return puts;
}

const show = (session: HostedSession = SIGNED_IN) =>
  render(<MemoryRouter><HostedProvider value={session}><SetupGate><FinishSetup /></SetupGate></HostedProvider></MemoryRouter>);

it('Projects says what setup has left while an optional step is neither done nor skipped', async () => {
  const puts = serve({ done: ['welcome', 'computer', 'models'], skipped: [] });
  show();
  const link = await screen.findByRole('link', { name: 'Finish setup' });
  expect(link).toHaveAttribute('href', '/setup');
  expect(screen.getByText(/More computers, First project/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Not now' }));
  await waitFor(() => expect(puts.at(-1)?.skipped).toEqual(['computers', 'tour']));
  expect(screen.queryByRole('link', { name: 'Finish setup' })).toBeNull();
});

it('says nothing once every step is done or skipped', async () => {
  serve({ done: ['welcome', 'computer', 'models', 'tour'], skipped: ['computers'] });
  const { container } = show();
  await waitFor(() => expect(globalThis.fetch).toHaveBeenCalled());
  await new Promise((r) => setTimeout(r, 20));
  expect(container).toBeEmptyDOMElement();
});

it('on your own computer it offers the practice project and tour', async () => {
  serve({ done: [], skipped: [] });
  show({ kind: 'local' });
  expect(await screen.findByRole('link', { name: 'Finish setup' })).toBeInTheDocument();
  expect(screen.getByText(/First project/)).toBeInTheDocument();
});
