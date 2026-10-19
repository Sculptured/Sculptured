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
  page();
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
