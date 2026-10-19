import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { SavedPage } from './SavedPage';
import { fakeFetch, meUser, publication, renderCommunity } from './testing';
import { YoursPage } from './YoursPage';

afterEach(() => vi.restoreAllMocks());
const signedIn = { signedIn: true, me: meUser() };

it('Saved lists saves and removes one', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/saved': () => [200, { blocks: [publication({ id: 'b1', title: 'Kept' })] }], 'DELETE /api/community/blocks/b1/save': () => [200, { removed: true }] });
  renderCommunity(<SavedPage />, { path: '/community/saved', state: signedIn });
  await userEvent.click(await screen.findByRole('button', { name: 'Remove save' }));
  expect(await screen.findByText('Nothing saved yet.')).toBeVisible();
  expect(calls).toContain('DELETE /api/community/blocks/b1/save');
});

it('Yours unpublishes after confirming and links to Publish', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/mine': () => [200, { blocks: [publication({ id: 'b2', title: 'Mine' })] }], 'DELETE /api/community/blocks/b2': () => [200, { unpublished: true }] });
  renderCommunity(<YoursPage />, { path: '/community/mine', state: signedIn });
  expect(await screen.findByRole('link', { name: 'Publish a group' })).toHaveAttribute('href', '/community/publish');
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Unpublish' }));
  expect(screen.getByText('Unpublish Mine? It disappears from Explore.')).toBeVisible();
  expect(calls).not.toContain('DELETE /api/community/blocks/b2');
  await user.click(screen.getByRole('button', { name: 'Yes, unpublish' }));
  await vi.waitFor(() => expect(calls).toContain('DELETE /api/community/blocks/b2'));
});

it('signed out, both ask to sign in', () => {
  renderCommunity(<SavedPage />, { path: '/community/saved' });
  expect(screen.getByText(/Sign in to see what you saved/)).toBeVisible();
});
