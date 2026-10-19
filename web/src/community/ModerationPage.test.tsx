import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { ModerationPage } from './ModerationPage';
import { fakeFetch, meUser, renderCommunity } from './testing';

afterEach(() => vi.restoreAllMocks());
const admin = { signedIn: true, me: meUser({ admin: true }) };
const blockReport = { id: 'r1', reason: 'Spam links', kind: 'block', block: { id: 'b1', title: 'Spammy' }, handle: null, created_at: '2026-10-19T00:00:00Z' };
const profileReport = { id: 'r2', reason: 'Offensive name', kind: 'profile', block: null, handle: 'spam', created_at: '2026-10-19T00:00:00Z' };

it('non-admins see why and nothing is fetched', () => {
  const { calls } = fakeFetch({});
  renderCommunity(<ModerationPage />, { path: '/community/moderation', state: { signedIn: true, me: meUser({ admin: false }) } });
  expect(screen.getByText('Only Community moderators can see this page.')).toBeVisible();
  expect(calls).toEqual([]);
});

it('hides a reported block and reloads the list', async () => {
  let open = [blockReport];
  const { calls, inits } = fakeFetch({ 'GET /api/community/reports': () => [200, { reports: open }], 'POST /api/community/blocks/b1/moderate': () => { open = []; return [200, { updated: true }]; } });
  renderCommunity(<ModerationPage />, { path: '/community/moderation', state: admin });
  expect(await screen.findByText('Spam links')).toHaveAttribute('data-raw-evidence');
  expect(screen.getByRole('link', { name: 'Spammy' })).toHaveAttribute('href', '/community/blocks/b1');
  await userEvent.click(screen.getByRole('button', { name: 'Hide block' }));
  expect(await screen.findByText('No open reports.')).toBeVisible();
  expect(JSON.parse(String(inits[calls.indexOf('POST /api/community/blocks/b1/moderate')]!.body))).toEqual({ hidden: true });
});

it('resets a reported profile’s picture and handle and can disable it', async () => {
  const { calls, inits } = fakeFetch({ 'GET /api/community/reports': () => [200, { reports: [profileReport] }], 'POST /api/community/users/spam/moderate': () => [200, { ok: true }] });
  renderCommunity(<ModerationPage />, { path: '/community/moderation', state: admin });
  expect(await screen.findByRole('link', { name: '@spam' })).toHaveAttribute('href', '/community/u/spam');
  const user = userEvent.setup();
  await user.click(screen.getByRole('checkbox', { name: 'Reset picture' }));
  await user.click(screen.getByRole('checkbox', { name: 'Reset handle' }));
  await user.click(screen.getByRole('checkbox', { name: 'Also disable this account' }));
  await user.click(screen.getByRole('button', { name: 'Apply' }));
  await vi.waitFor(() => expect(calls).toContain('POST /api/community/users/spam/moderate'));
  expect(JSON.parse(String(inits[calls.indexOf('POST /api/community/users/spam/moderate')]!.body))).toEqual({ reset: ['avatar', 'handle'], disable: true });
});

it('Apply never re-enables an account by itself', async () => {
  const { calls, inits } = fakeFetch({ 'GET /api/community/reports': () => [200, { reports: [profileReport] }], 'POST /api/community/users/spam/moderate': () => [200, { ok: true }] });
  renderCommunity(<ModerationPage />, { path: '/community/moderation', state: admin });
  const user = userEvent.setup();
  await user.click(await screen.findByRole('checkbox', { name: 'Reset bio' }));
  await user.click(screen.getByRole('button', { name: 'Apply' }));
  await vi.waitFor(() => expect(calls).toContain('POST /api/community/users/spam/moderate'));
  expect(JSON.parse(String(inits[calls.indexOf('POST /api/community/users/spam/moderate')]!.body))).toEqual({ reset: ['bio'] });
});
