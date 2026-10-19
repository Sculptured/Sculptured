import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { PeoplePage } from './PeoplePage';
import { fakeFetch, profile, renderCommunity } from './testing';

afterEach(() => vi.restoreAllMocks());

it('lists people with their handle and pages with More', async () => {
  const first = Array.from({ length: 24 }, (_, i) => profile({ handle: `p${i}`, name: `Person ${i}`, bio: i === 0 ? 'Builds review teams.\nAnd more.' : '' }));
  const { calls } = fakeFetch({
    'GET /api/community/people?q=&offset=0': () => [200, { people: first, next: 24 }],
    'GET /api/community/people?q=&offset=24': () => [200, { people: [profile({ handle: 'last', name: 'Last Person' })], next: null }],
    'GET /api/community/people?q=ana&offset=0': () => [200, { people: [], next: null }],
  });
  renderCommunity(<PeoplePage />, { path: '/community/people' });
  expect(await screen.findByRole('link', { name: /Person 0/ })).toHaveAttribute('href', '/community/u/p0');
  expect(screen.getByText('@p0')).toBeVisible();
  expect(screen.getByText('Builds review teams.')).toBeVisible();
  expect(screen.queryByText('And more.')).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'More' }));
  expect(await screen.findByRole('link', { name: /Last Person/ })).toBeVisible();
  expect(screen.queryByRole('button', { name: 'More' })).toBeNull();
  await userEvent.type(screen.getByRole('searchbox', { name: 'Search people' }), 'ana{Enter}');
  expect(await screen.findByText('Nobody matches that search.')).toBeVisible();
  expect(calls.at(-1)).toBe('GET /api/community/people?q=ana&offset=0');
});
