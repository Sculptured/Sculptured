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

const local: HostedSession = { kind: 'local' };
const linkOff = { enabled: false, origin: null, signed_in: false, account: null };
const linkOn = (signed_in = false) => ({ enabled: true, origin: 'https://app.usesculptured.com', signed_in, account: signed_in ? { name: 'You Example', handle: null } : null });

it('this computer: the community section asks first and fetches nothing while off', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  const { calls } = fakeFetch({ 'GET /api/community-link': () => [200, linkOff] });
  at('/community', local);
  expect(await screen.findByRole('heading', { name: 'Turn on Community' })).toBeInTheDocument();
  expect(calls.filter((c) => c.includes('/api/community/'))).toEqual([]);
  resetCommunityLink();
});

it('this computer: Learn needs nothing turned on', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  const { calls } = fakeFetch({});
  at('/community/learn', local);
  expect(await screen.findByRole('heading', { name: 'Designing teams of agents' })).toBeVisible();
  expect(calls).toEqual([]);
});

it('this computer: turning on opens Explore, where a block can be added to a project signed out', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  let on = false;
  fakeFetch({
    'GET /api/community-link': () => [200, on ? linkOn() : linkOff],
    'POST /api/community-link/enable': () => { on = true; return [200, linkOn()]; },
    'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [publication()] }],
    'GET /api/group-blocks': () => [200, []],
  });
  at('/community', local);
  (await screen.findByRole('button', { name: 'Turn on and browse' })).click();
  expect(await screen.findByRole('searchbox', { name: 'Search blocks' })).toBeVisible();
  const add = await screen.findByRole('button', { name: 'Add to project' });
  expect(add).toBeEnabled();
  expect(screen.queryByText('Sign in to add it to a project or your Library.')).toBeNull();
  expect(screen.getByText('Sign in to save')).toBeVisible();
  resetCommunityLink();
});

it('this computer: "Turn on and sign in" shows the code to approve above the page', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  let on = false;
  fakeFetch({
    'GET /api/community-link': () => [200, on ? linkOn() : linkOff],
    'POST /api/community-link/enable': () => { on = true; return [200, linkOn()]; },
    'POST /api/community-link/pair': () => [200, { user_code: 'ABCD-EFGH', verify_url: 'https://app.usesculptured.com/connect?code=ABCD-EFGH', expires_in: 600 }],
    'GET /api/community-link/pair': () => [200, { status: 'pending' }],
    'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [] }],
  });
  at('/community', local);
  (await screen.findByRole('button', { name: 'Turn on and sign in' })).click();
  expect(await screen.findByText('ABCD-EFGH')).toBeVisible();
  expect(screen.getByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', 'https://app.usesculptured.com/connect?code=ABCD-EFGH');
  resetCommunityLink();
});

it('this computer: signed in, the profile loads; a refused key shows signed out', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  let signed = true;
  fakeFetch({
    'GET /api/community-link': () => [200, linkOn(signed)],
    'GET /api/community/me': () => [200, { user: meUser() }],
    'GET /api/community/saved': () => { signed = false; return [401, { error: { code: 'device_unknown', message: 'x' } }]; },
  });
  at('/community/saved', local);
  expect(await screen.findByText(/Your Community sign-in was removed|Sign in to see what you saved/)).toBeVisible();
  expect(await screen.findByText(/Sign in to see what you saved/)).toBeVisible();
  resetCommunityLink();
});

it('this computer: once the code is approved the panel says who you are signed in as', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let on = false;
  let approved = false;
  fakeFetch({
    'GET /api/community-link': () => [200, on ? linkOn(approved) : linkOff],
    'POST /api/community-link/enable': () => { on = true; return [200, linkOn()]; },
    'POST /api/community-link/pair': () => [200, { user_code: 'ABCD-EFGH', verify_url: 'https://app.usesculptured.com/connect?code=ABCD-EFGH', expires_in: 600 }],
    'GET /api/community-link/pair': () => { approved = true; return [200, { status: 'approved' }]; },
    'GET /api/community/me': () => [200, { user: meUser() }],
    'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [] }],
  });
  at('/community', local);
  (await screen.findByRole('button', { name: 'Turn on and sign in' })).click();
  expect(await screen.findByText('ABCD-EFGH')).toBeVisible();
  await vi.advanceTimersByTimeAsync(2100);
  expect(await screen.findByText('Signed in as You Example')).toBeVisible();
  vi.useRealTimers();
  resetCommunityLink();
});

it('this computer: the Turn on page offers an address an older version saved', async () => {
  const { resetCommunityLink } = await import('./link');
  resetCommunityLink();
  fakeFetch({ 'GET /api/community-link': () => [200, { ...linkOff, suggested_origin: 'https://sculptured.my.org' }] });
  at('/community', local);
  expect(await screen.findByLabelText('Server address')).toHaveValue('https://sculptured.my.org');
  resetCommunityLink();
});
