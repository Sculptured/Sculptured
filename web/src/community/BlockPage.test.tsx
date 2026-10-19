import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { COMMUNITY_WORDS } from '../vocab';
import { projectCopy } from './BlockActions';
import { BlockPage } from './BlockPage';
import { fakeFetch, meUser, publication, renderCommunity } from './testing';
import type { Publication } from '../generated/community/Publication';

beforeEach(() => { HTMLDialogElement.prototype.showModal = function () { this.open = true; }; });
afterEach(() => vi.restoreAllMocks());

const page = (state = {}) => renderCommunity(<BlockPage />, { path: '/community/blocks/b1', route: '/community/blocks/:id', state });
const signedIn = { signedIn: true, me: meUser() };

it('shows the block, who published it, and a note that publishing is not verification', async () => {
  fakeFetch({ 'GET /api/community/blocks/b1': () => [200, publication()] });
  page();
  expect(await screen.findByRole('heading', { name: 'Review pair' })).toBeVisible();
  expect(screen.getByRole('link', { name: /Ana Lee/ })).toHaveAttribute('href', '/community/u/ana');
  expect(screen.getByText(COMMUNITY_WORDS.supplied)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Unpublish' })).toBeNull();
});

it('Add to project opens the review step with a copy that has no folders', async () => {
  const pub = publication();
  pub.content.draft.folder = '/secret';
  fakeFetch({ 'GET /api/community/blocks/b1': () => [200, pub] });
  page(signedIn);
  await userEvent.click(await screen.findByRole('button', { name: 'Add to project' }));
  expect(await screen.findByRole('dialog', { name: 'Add Review pair to project' })).toBeVisible();
  expect(projectCopy(pub)?.draft.folder ?? null).toBeNull();
  expect(projectCopy(pub)?.draft.agents.every((a) => a.folder === null)).toBe(true);
});

it('a block of another kind says it opens in a later version', async () => {
  fakeFetch({ 'GET /api/community/blocks/b1': () => [200, publication({ kind: 'component' } as Partial<Publication>)] });
  page();
  expect(await screen.findByText(COMMUNITY_WORDS.laterVersion)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Add to project' })).toBeNull();
});

it('its publisher can unpublish it', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/blocks/b1': () => [200, publication()], 'DELETE /api/community/blocks/b1': () => [200, { unpublished: true }] });
  page({ signedIn: true, me: meUser({ handle: 'ana' }) });
  await userEvent.click(await screen.findByRole('button', { name: 'Unpublish' }));
  expect(await screen.findByText('Unpublished. It no longer shows in Explore.')).toBeVisible();
  expect(calls).toContain('DELETE /api/community/blocks/b1');
});

it('reporting needs a reason and sends it', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/blocks/b1': () => [200, publication()], 'POST /api/community/blocks/b1/report': () => [200, { reported: true }] });
  page({ signedIn: true, me: meUser({ handle: 'bea' }) });
  await userEvent.click(await screen.findByText('Report this block'));
  await userEvent.type(screen.getByRole('textbox', { name: 'What is wrong with it?' }), 'Spam links everywhere');
  await userEvent.click(screen.getByRole('button', { name: 'Send report' }));
  expect(await screen.findByText('Thanks. A moderator will look at it.')).toBeVisible();
  expect(calls).toContain('POST /api/community/blocks/b1/report');
});

it('Save to Library saves a copy once, then says it is there; a newer version offers an update', async () => {
  let library: unknown[] = [];
  const { calls } = fakeFetch({
    'GET /api/community/blocks/b1': () => [200, publication({ version: 2 })],
    'GET /api/group-blocks': () => [200, library],
    'PUT /api/group-blocks/community-b1': (init) => { const b = { ...JSON.parse(String(init!.body)).block, revision: 1 }; library = [b]; return [200, b]; },
  });
  page(signedIn);
  await userEvent.click(await screen.findByRole('button', { name: 'Save to Library' }));
  expect(await screen.findByRole('button', { name: 'In your Library' })).toBeDisabled();
  expect(screen.getByText('This version is already saved.')).toBeVisible();
  expect(calls.filter((c) => c.startsWith('PUT'))).toEqual(['PUT /api/group-blocks/community-b1']);
});

it('offers Update to v2 when the Library has v1', async () => {
  const v1 = { ...publication().content, id: 'community-b1', revision: 3, source: { community_block: 'b1', version: 1 } };
  fakeFetch({ 'GET /api/community/blocks/b1': () => [200, publication({ version: 2 })], 'GET /api/group-blocks': () => [200, [v1]] });
  page(signedIn);
  expect(await screen.findByRole('button', { name: 'Update to v2' })).toBeEnabled();
});

it('signed out, adding to a project or the Library says to sign in first', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/blocks/b1': () => [200, publication()] });
  page();
  expect(await screen.findByRole('button', { name: 'Add to project' })).toBeDisabled();
  expect(screen.getByText('Sign in to add it to a project or your Library.')).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Save to Library' })).toBeNull();
  expect(calls).not.toContain('GET /api/group-blocks');
});
