import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { COMMUNITY_ERROR_WORDS } from '../errors';
import { fakeFetch, meUser, renderCommunity } from './testing';
import { YourProfilePage } from './YourProfilePage';

afterEach(() => vi.restoreAllMocks());
const page = (me = meUser({ handle: 'you-ex', bio: '' })) => renderCommunity(<YourProfilePage />, { path: '/community/me', state: { signedIn: true, me } });

it('a handle taken while editing shows under Handle and keeps the other fields', async () => {
  fakeFetch({ 'PATCH /api/community/me': () => [409, { error: { code: 'handle_taken', message: 'x' } }] });
  page();
  const user = userEvent.setup();
  await user.clear(screen.getByLabelText('Handle'));
  await user.type(screen.getByLabelText('Handle'), 'taken');
  await user.type(screen.getByLabelText('Bio'), 'Builds review teams.');
  await user.click(screen.getByRole('button', { name: 'Save profile' }));
  const handle = screen.getByLabelText('Handle');
  await vi.waitFor(() => expect(handle).toHaveAccessibleDescription(expect.stringContaining('Someone already has that handle')));
  expect(handle).toHaveFocus();
  expect(screen.getByLabelText('Bio')).toHaveValue('Builds review teams.');
});

it('sends the handle only when it changed, and the compact setting with it', async () => {
  const reload = vi.fn(async () => {});
  const { inits } = fakeFetch({ 'PATCH /api/community/me': () => [200, { user: meUser() }] });
  renderCommunity(<YourProfilePage />, { path: '/community/me', state: { signedIn: true, me: meUser({ handle: 'you-ex' }), reloadMe: reload } });
  const user = userEvent.setup();
  await user.type(screen.getByLabelText('Bio'), 'Hi');
  await user.click(screen.getByRole('checkbox', { name: 'Compact block cards' }));
  await user.click(screen.getByRole('button', { name: 'Save profile' }));
  expect(await screen.findByText('Saved.')).toBeVisible();
  const body = JSON.parse(String(inits[0]!.body));
  expect(body).not.toHaveProperty('handle');
  expect(body).toMatchObject({ bio: 'Hi', settings: { compact: false }, links: [] });
  expect(reload).toHaveBeenCalled();
});

it('checks handle rules before sending', async () => {
  const { calls } = fakeFetch({});
  page();
  const user = userEvent.setup();
  await user.clear(screen.getByLabelText('Handle'));
  await user.type(screen.getByLabelText('Handle'), 'support');
  await user.click(screen.getByRole('button', { name: 'Save profile' }));
  expect(screen.getByLabelText('Handle')).toHaveAccessibleDescription(expect.stringContaining(COMMUNITY_ERROR_WORDS.handle_reserved));
  expect(calls).toEqual([]);
});

it('signed out: asks to sign in', () => {
  renderCommunity(<YourProfilePage />, { path: '/community/me' });
  expect(screen.getByText('Sign in to edit your profile.')).toBeVisible();
});

it('offers a picture only once there is a handle, and says why', () => {
  page(meUser({ handle: null, bio: '' }));
  expect(screen.queryByLabelText('Choose a picture')).toBeNull();
  expect(screen.getByText(/Choose a handle first/)).toBeInTheDocument();
});

it('a refused link is described on each link box and announced', async () => {
  fakeFetch({ 'PATCH /api/community/me': () => [400, { error: { code: 'link_invalid', message: 'Links must start with https://' } }] });
  page();
  const user = userEvent.setup();
  await user.type(screen.getByLabelText('Link 1'), 'http://a.example');
  await user.click(screen.getByRole('button', { name: 'Save profile' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('https://');
  expect(screen.getByLabelText('Link 1')).toHaveAccessibleDescription(expect.stringContaining('https://'));
  expect(screen.getByLabelText('Link 1')).toBeInvalid();
});
