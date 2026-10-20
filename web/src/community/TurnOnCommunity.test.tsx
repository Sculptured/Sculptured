import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { COMMUNITY_ERROR_WORDS } from '../errors';
import { openRigWords } from '../test-vocab';
import { COMMUNITY_WORDS } from '../vocab';
import { fakeFetch } from './testing';
import { TurnOnCommunity } from './TurnOnCommunity';

afterEach(() => vi.restoreAllMocks());

it('says what is sent and what never leaves, and turns on only when asked', async () => {
  const { calls } = fakeFetch({ 'POST /api/community-link/enable': () => [200, { enabled: true, origin: 'https://app.usesculptured.com', signed_in: false, account: null }] });
  const onDone = vi.fn();
  render(<MemoryRouter><TurnOnCommunity onDone={onDone} /></MemoryRouter>);
  for (const t of [COMMUNITY_WORDS.optIn.title, ...COMMUNITY_WORDS.optIn.sends, COMMUNITY_WORDS.optIn.never]) expect(screen.getByText(t)).toBeInTheDocument();
  expect(calls).toEqual([]);
  await userEvent.click(screen.getByRole('button', { name: 'Turn on and browse' }));
  expect(calls).toEqual(['POST /api/community-link/enable']);
  expect(onDone).toHaveBeenCalledWith(false);
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});

it('turns on and asks to sign in, with another server when one is given', async () => {
  const { inits } = fakeFetch({ 'POST /api/community-link/enable': () => [200, { enabled: true, origin: 'https://sculptured.my.org', signed_in: false, account: null }] });
  const onDone = vi.fn();
  render(<MemoryRouter><TurnOnCommunity onDone={onDone} /></MemoryRouter>);
  await userEvent.click(screen.getByText(COMMUNITY_WORDS.optIn.otherServer));
  await userEvent.type(screen.getByLabelText('Server address'), 'https://sculptured.my.org');
  await userEvent.click(screen.getByRole('button', { name: 'Turn on and sign in' }));
  expect(JSON.parse(String(inits[0]?.body))).toEqual({ origin: 'https://sculptured.my.org' });
  expect(onDone).toHaveBeenCalledWith(true);
});

it('refuses a plain http server address in words', async () => {
  fakeFetch({ 'POST /api/community-link/enable': () => [400, { error: { code: 'community_origin_invalid', message: 'Use an https:// address' } }] });
  const onDone = vi.fn();
  render(<MemoryRouter><TurnOnCommunity onDone={onDone} /></MemoryRouter>);
  await userEvent.click(screen.getByText(COMMUNITY_WORDS.optIn.otherServer));
  await userEvent.type(screen.getByLabelText('Server address'), 'http://evil.example');
  await userEvent.click(screen.getByRole('button', { name: 'Turn on and browse' }));
  expect(await screen.findByText(COMMUNITY_ERROR_WORDS.community_origin_invalid)).toBeInTheDocument();
  expect(onDone).not.toHaveBeenCalled();
});
