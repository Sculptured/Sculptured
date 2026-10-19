import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useLocation } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { COMMUNITY_ERROR_WORDS } from '../errors';
import { PublishPage } from './PublishPage';
import { fakeFetch, meUser, publication, renderCommunity } from './testing';
import type { MeUser } from '../generated/community/MeUser';

afterEach(() => vi.restoreAllMocks());
function Where() { return <p data-testid="where">{useLocation().pathname}</p>; }

const libraryBlock = () => {
  const b = publication().content;
  return { ...b, id: 'lib-1', name: 'Review pair', draft: { ...b.draft, agents: b.draft.agents.map((a) => ({ ...a, folder: '/secret' })) }, source: { community_block: 'x', version: 1 } };
};

function setup(me: MeUser, publish: () => [number, unknown]) {
  let current = me;
  const reload = vi.fn(async () => {});
  const r = fakeFetch({
    'GET /api/group-blocks': () => [200, [libraryBlock()]],
    'PATCH /api/community/me': (init) => { current = { ...current, handle: JSON.parse(String(init!.body)).handle }; return [200, { user: current }]; },
    'POST /api/community/blocks': publish,
  });
  return { ...r, reload };
}

async function fillAndPublish() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole('radio', { name: /Review pair/ }));
  await user.type(screen.getByLabelText('Title'), 'My pair');
  await user.type(screen.getByLabelText('Description'), 'Writes and reviews.');
  await user.click(screen.getByRole('checkbox', { name: /I reviewed every prompt/ }));
  await user.click(screen.getByRole('button', { name: 'Publish publicly' }));
}

it('asks for a handle first, then publishes a reviewed Library group without private fields', async () => {
  const { calls, inits, reload } = setup(meUser({ handle: null, name: 'You Example' }), () => [200, { id: 'new1', version: 1 }]);
  renderCommunity(<><PublishPage /><Where /></>, { path: '/community/publish', route: '/community/*', state: { signedIn: true, me: meUser({ handle: null, name: 'You Example' }), reloadMe: reload } });
  expect(await screen.findByRole('heading', { name: 'Choose your handle' })).toBeVisible();
  expect(screen.getByLabelText('Handle')).toHaveValue('you-example');
  await userEvent.click(screen.getByRole('button', { name: 'Use this handle' }));
  await fillAndPublish();
  await vi.waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/community/blocks/new1'));
  const body = JSON.parse(String(inits[calls.indexOf('POST /api/community/blocks')]!.body));
  expect(body.content.draft.agents[0].folder).toBeNull();
  expect(body.content.source).toBeUndefined();
  expect(body).toMatchObject({ title: 'My pair', reviewed: true, license: 'MIT' });
});

it('Publish stays disabled until a group is chosen and reviewed, and says why', async () => {
  setup(meUser(), () => [200, {}]);
  renderCommunity(<PublishPage />, { path: '/community/publish', state: { signedIn: true, me: meUser() } });
  expect(await screen.findByRole('button', { name: 'Publish publicly' })).toBeDisabled();
  expect(screen.getByText('Choose a group and confirm the review to publish.')).toBeVisible();
});

it('a 409 handle_required brings back the handle step; publish_limit is said in words', async () => {
  let answer: [number, unknown] = [409, { error: { code: 'handle_required', message: 'x' } }];
  setup(meUser(), () => answer);
  const { unmount } = renderCommunity(<PublishPage />, { path: '/community/publish', state: { signedIn: true, me: meUser() } });
  await fillAndPublish();
  expect(await screen.findByRole('heading', { name: 'Choose your handle' })).toBeVisible();
  unmount();
  answer = [429, { error: { code: 'publish_limit', message: 'x' } }];
  renderCommunity(<PublishPage />, { path: '/community/publish', state: { signedIn: true, me: meUser() } });
  await fillAndPublish();
  expect(await screen.findByText(COMMUNITY_ERROR_WORDS.publish_limit)).toBeVisible();
});

it('signed out, it asks to sign in', () => {
  renderCommunity(<PublishPage />, { path: '/community/publish' });
  expect(screen.getByText(/Sign in to publish/)).toBeVisible();
});
