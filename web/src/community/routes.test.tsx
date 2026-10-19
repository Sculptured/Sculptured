import { render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { CommunityFrame } from './CommunityFrame';
import { CommunitySection } from './routes';
import { fakeFetch, meUser, profile, publication } from './testing';

afterEach(() => vi.restoreAllMocks());

const signedIn: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana Lee', email: 'a@x', role: 'member' }, csrf: 'c' };

function at(path: string, session: HostedSession = signedIn) {
  return render(
    <HostedProvider value={session}>
      <MemoryRouter initialEntries={[path]}><Routes><Route path="/community/*" element={<CommunitySection />} /></Routes></MemoryRouter>
    </HostedProvider>,
  );
}

function community(me = meUser()) {
  return fakeFetch({
    'GET /api/community/me': () => [200, { user: me }],
    'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [] }],
    'GET /api/community/people?q=&offset=0': () => [200, { people: [], next: null }],
    'GET /api/community/users/ana': () => [200, { profile: profile(), blocks: [] }],
    'GET /api/community/saved': () => [200, { blocks: [] }],
    'GET /api/community/mine': () => [200, { blocks: [] }],
    'GET /api/community/reports': () => [200, { reports: [] }],
    'GET /api/community/blocks/b1': () => [200, publication()],
    'GET /api/group-blocks': () => [200, []],
  });
}

it.each([
  ['/community', () => screen.findByRole('searchbox', { name: 'Search blocks' })],
  ['/community/people', () => screen.findByRole('searchbox', { name: 'Search people' })],
  ['/community/u/ana', () => screen.findByRole('heading', { name: 'Ana Lee' })],
  ['/community/me', () => screen.findByRole('heading', { name: 'Your profile' })],
  ['/community/saved', () => screen.findByText('Nothing saved yet.')],
  ['/community/mine', () => screen.findByRole('link', { name: 'Publish a group' })],
  ['/community/publish', () => screen.findByRole('heading', { name: 'Publish a group' })],
  ['/community/moderation', () => screen.findByText('No open reports.')],
  ['/community/blocks/b1', () => screen.findByRole('heading', { name: 'Review pair' })],
  ['/community/learn', () => screen.findByRole('heading', { name: 'Designing teams of agents' })],
  ['/community/explore', () => screen.findByRole('searchbox', { name: 'Search blocks' })],
  ['/community/whatever/x', () => screen.findByRole('searchbox', { name: 'Search blocks' })],
])('hosted: %s opens its page', async (path, find) => {
  community(meUser({ admin: true }));
  at(path);
  expect(await find()).toBeVisible();
});

it('the tab row is Explore · People · Saved · Yours, and Moderation only for admins', async () => {
  community(meUser({ admin: false }));
  const { unmount } = at('/community');
  const tabs = await screen.findByRole('navigation', { name: 'Community' });
  await vi.waitFor(() => expect(within(tabs).getAllByRole('link').map((l) => l.textContent)).toEqual(['Explore', 'People', 'Saved', 'Yours']));
  unmount();
  vi.restoreAllMocks();
  community(meUser({ admin: true }));
  at('/community');
  const admin = await screen.findByRole('navigation', { name: 'Community' });
  expect(await within(admin).findByRole('link', { name: 'Moderation' })).toHaveAttribute('href', '/community/moderation');
});

it('signed out, Community is browsable and reads no profile', async () => {
  const { calls } = community();
  at('/community', { kind: 'signed-out' });
  expect(await screen.findByRole('searchbox', { name: 'Search blocks' })).toBeVisible();
  expect(calls).not.toContain('GET /api/community/me');
});

it('the self-run app keeps its own Community page for now', async () => {
  const { calls } = fakeFetch({});
  at('/community', { kind: 'local' });
  expect(await screen.findByRole('heading', { name: 'Community', level: 1 })).toBeVisible();
  expect(calls.filter((c) => c.startsWith('GET /api/community/me'))).toEqual([]);
});

it('signed-out visitors get a light frame with Sign in, not the app', () => {
  render(<MemoryRouter initialEntries={['/community']}><Routes><Route element={<CommunityFrame />}><Route path="/community" element={<p>inside</p>} /></Route></Routes></MemoryRouter>);
  expect(screen.getByRole('link', { name: 'Sign in' })).toHaveAttribute('href', '/login');
  expect(screen.getByText('inside')).toBeVisible();
});

it('opening Publish directly waits for your profile, then skips the handle step when you have one', async () => {
  let release: () => void = () => {};
  const gate = new Promise<void>((r) => { release = r; });
  fakeFetch({
    'GET /api/community/me': () => new Response(null, { status: 599 }),
    'GET /api/group-blocks': () => [200, []],
  });
  vi.mocked(globalThis.fetch).mockImplementation(async (url) => {
    if (String(url) === '/api/community/me') { await gate; return new Response(JSON.stringify({ user: meUser({ handle: 'ana' }) }), { status: 200 }); }
    return new Response(JSON.stringify([]), { status: 200 });
  });
  at('/community/publish');
  expect(await screen.findByText('Loading your profile…')).toBeVisible();
  expect(screen.queryByText(/Sign in to publish/)).toBeNull();
  release();
  expect(await screen.findByText(/Your Library has no groups yet/)).toBeVisible();
  expect(screen.queryByRole('heading', { name: 'Choose your handle' })).toBeNull();
});

it('a profile that cannot be read says why instead of asking you to sign in', async () => {
  fakeFetch({ 'GET /api/community/me': () => [403, { error: { code: 'account_disabled', message: 'x' } }] });
  at('/community/me');
  expect(await screen.findByRole('alert')).toHaveTextContent('This Community account is disabled.');
  expect(screen.queryByText(/Sign in to edit/)).toBeNull();
});
