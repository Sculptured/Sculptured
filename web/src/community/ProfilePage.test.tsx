import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useLocation } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { ERROR_WORDS } from '../errors';
import { openRigWords } from '../test-vocab';
import { ProfilePage } from './ProfilePage';
import { fakeFetch, meUser, profile, publication, renderCommunity } from './testing';

afterEach(() => vi.restoreAllMocks());

function Where() { return <p data-testid="where">{useLocation().pathname}</p>; }
const page = (handle: string, state = {}) => renderCommunity(<><ProfilePage /><Where /></>, { path: `/community/u/${handle}`, route: '/community/u/:handle', state });

it('shows a profile with only the fields it has, links as host names, and its blocks', async () => {
  fakeFetch({ 'GET /api/community/users/ana': () => [200, { profile: profile({ bio: '', links: ['https://ana.dev/x'], counts: { published: 2, saved_by_others: 5 } }), blocks: [publication()] }] });
  page('ana');
  expect(await screen.findByRole('heading', { name: 'Ana Lee' })).toBeVisible();
  const link = screen.getByRole('link', { name: 'ana.dev' });
  expect(link).toHaveAttribute('href', 'https://ana.dev/x');
  expect(link.getAttribute('rel')).toMatch(/nofollow/);
  expect(link.getAttribute('rel')).toMatch(/noopener/);
  expect(screen.getByText('2 blocks published · saved 5 times')).toBeVisible();
  expect(screen.getByText(/@ana · Joined /)).toBeVisible();
  expect(document.querySelector('.cm-bio')).toBeNull();
  expect(screen.getByRole('link', { name: 'Review pair' })).toBeVisible();
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});

it('never links a non-https link', async () => {
  fakeFetch({ 'GET /api/community/users/ana': () => [200, { profile: profile({ links: ['javascript:alert(1)'] }), blocks: [] }] });
  page('ana');
  await screen.findByRole('heading', { name: 'Ana Lee' });
  expect(screen.queryByRole('link', { name: /javascript|alert/ })).toBeNull();
});

it('an old handle lands on the new address', async () => {
  const moved = () => [200, { profile: profile({ handle: 'ana' }), blocks: [] }] as [number, unknown];
  fakeFetch({ 'GET /api/community/users/old-ana': moved, 'GET /api/community/users/ana': moved });
  page('old-ana');
  await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/community/u/ana'));
  expect(await screen.findByRole('heading', { name: 'Ana Lee' })).toBeVisible();
});

it('an unknown handle says so in words', async () => {
  fakeFetch({});
  page('nobody');
  expect(await screen.findByRole('alert')).toHaveTextContent(ERROR_WORDS.not_found);
});

it('signed in, a profile can be reported', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/users/ana': () => [200, { profile: profile(), blocks: [] }], 'POST /api/community/users/ana/report': () => [200, { ok: true }] });
  page('ana', { signedIn: true, me: meUser({ handle: 'bea' }) });
  await userEvent.click(await screen.findByText('Report this profile'));
  await userEvent.type(screen.getByRole('textbox', { name: 'What is wrong with it?' }), 'Offensive name');
  await userEvent.click(screen.getByRole('button', { name: 'Send report' }));
  expect(await screen.findByText('Thanks. A moderator will look at it.')).toBeVisible();
  expect(calls).toContain('POST /api/community/users/ana/report');
});
