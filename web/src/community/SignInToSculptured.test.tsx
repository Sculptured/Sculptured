import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { openRigWords } from '../test-vocab';
import { resetCommunityLink } from './link';
import { SignInToSculptured } from './SignInToSculptured';
import { fakeFetch } from './testing';

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); resetCommunityLink(); });

const started = (verify = 'http://stub/connect?code=ABCD-EFGH') => () => [200, { user_code: 'ABCD-EFGH', verify_url: verify, expires_in: 600 }] as [number, unknown];

function server(statuses: string[], signedIn = { name: 'You Example', handle: null }) {
  let polls = 0;
  let approved = false;
  return fakeFetch({
    'GET /api/community-link': () => [200, { enabled: true, origin: 'http://stub', signed_in: approved, account: approved ? signedIn : null }],
    'POST /api/community-link/pair': started(),
    'GET /api/community-link/pair': () => {
      const status = statuses[Math.min(polls++, statuses.length - 1)];
      approved = status === 'approved';
      return [200, { status }];
    },
  });
}

it('shows the code and the sign-in link, then the signed-in name once approved', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  server(['pending', 'approved']);
  const onSignedIn = vi.fn();
  render(<MemoryRouter><SignInToSculptured onSignedIn={onSignedIn} /></MemoryRouter>);
  expect(await screen.findByText('ABCD-EFGH')).toBeInTheDocument();
  expect(await screen.findByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', 'http://stub/connect?code=ABCD-EFGH');
  expect(screen.getByText('Waiting for you to approve…')).toBeInTheDocument();
  await vi.advanceTimersByTimeAsync(4000);
  expect(await screen.findByText('Signed in as You Example')).toBeInTheDocument();
  expect(onSignedIn).toHaveBeenCalled();
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});

it('an expired or turned-down code says so and offers a new one', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const { calls } = server(['expired']);
  render(<MemoryRouter><SignInToSculptured /></MemoryRouter>);
  await screen.findByText('ABCD-EFGH');
  await vi.advanceTimersByTimeAsync(2100);
  expect(await screen.findByText('The code expired before it was approved.')).toBeInTheDocument();
  vi.useRealTimers();
  await userEvent.click(screen.getByRole('button', { name: 'Get a new code' }));
  expect(calls.filter((c) => c === 'POST /api/community-link/pair')).toHaveLength(2);
});

it('a turned-down code says so', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  server(['denied']);
  render(<MemoryRouter><SignInToSculptured /></MemoryRouter>);
  await screen.findByText('ABCD-EFGH');
  await vi.advanceTimersByTimeAsync(2100);
  expect(await screen.findByText('The sign-in was turned down.')).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Get a new code' })).toBeInTheDocument();
});

it('hides a sign-in link that points away from the Sculptured server', async () => {
  fakeFetch({
    'GET /api/community-link': () => [200, { enabled: true, origin: 'http://stub', signed_in: false, account: null }],
    'POST /api/community-link/pair': started('https://evil/x'),
    'GET /api/community-link/pair': () => [200, { status: 'pending' }],
  });
  render(<MemoryRouter><SignInToSculptured /></MemoryRouter>);
  expect(await screen.findByText('ABCD-EFGH')).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: 'Open the sign-in page' })).toBeNull();
});
