import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { openRigWords } from '../test-vocab';
import { Avatar } from './Avatar';
import { ExplorePage } from './ExplorePage';
import { fakeFetch, meUser, publication, renderCommunity } from './testing';

afterEach(() => vi.restoreAllMocks());

it('searches blocks and shows each author with a link to their profile', async () => {
  const { calls } = fakeFetch({
    'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [publication()] }],
    'GET /api/community/blocks?q=review&offset=0': () => [200, { blocks: [] }],
  });
  renderCommunity(<ExplorePage />, { path: '/community' });
  expect(await screen.findByRole('link', { name: /Ana Lee/ })).toHaveAttribute('href', '/community/u/ana');
  expect(screen.getByRole('link', { name: 'Review pair' })).toHaveAttribute('href', '/community/blocks/b1');
  await userEvent.type(screen.getByRole('searchbox', { name: 'Search blocks' }), 'review{Enter}');
  await screen.findByText('No blocks match that search.');
  expect(calls.filter((c) => c.startsWith('GET /api/community/blocks')).at(-1)).toBe('GET /api/community/blocks?q=review&offset=0');
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});

it('signed out: Save says why it is disabled; signed in it saves', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [publication()] }], 'POST /api/community/blocks/b1/save': () => [200, { saved: true }] });
  const { unmount } = renderCommunity(<ExplorePage />, { path: '/community' });
  expect(await screen.findByRole('button', { name: 'Save' })).toBeDisabled();
  expect(screen.getByText('Sign in to save')).toBeVisible();
  unmount();
  renderCommunity(<ExplorePage />, { path: '/community', state: { signedIn: true, me: meUser() } });
  await userEvent.click(await screen.findByRole('button', { name: 'Save' }));
  expect(await screen.findByRole('button', { name: 'Remove save' })).toBeEnabled();
  expect(calls).toContain('POST /api/community/blocks/b1/save');
});

it('a Community that cannot be reached says so in plain words, and Retry tries again', async () => {
  let n = 0;
  fakeFetch({ 'GET /api/community/blocks?q=&offset=0': () => (n++ === 0 ? [503, { error: { code: 'community_unavailable', message: 'x' } }] : [200, { blocks: [publication()] }]) });
  renderCommunity(<ExplorePage />, { path: '/community' });
  expect(await screen.findByRole('alert')).toHaveTextContent('Community is not available on this Sculptured server right now.');
  await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
  expect(await screen.findByRole('link', { name: 'Review pair' })).toBeVisible();
});

it('pages through 24 blocks at a time', async () => {
  const many = Array.from({ length: 24 }, (_, i) => publication({ id: `b${i}`, title: `Block ${i}` }));
  const { calls } = fakeFetch({ 'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: many }], 'GET /api/community/blocks?q=&offset=24': () => [200, { blocks: [publication({ id: 'last', title: 'Last one' })] }] });
  renderCommunity(<ExplorePage />, { path: '/community' });
  await userEvent.click(await screen.findByRole('button', { name: 'Next' }));
  expect(await screen.findByRole('link', { name: 'Last one' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
  expect(calls.filter((c) => c.startsWith('GET /api/community/blocks')).at(-1)).toBe('GET /api/community/blocks?q=&offset=24');
  expect(calls.filter((c) => c === 'GET /api/group-blocks').length).toBeLessThanOrEqual(2);
});

it('a picture from anywhere else falls back to initials', () => {
  render(<Avatar name="Ada Lovelace" src="https://evil.example/a.png" />);
  expect(document.querySelector('img')).toBeNull();
  expect(screen.getByText('AL')).toBeVisible();
});
