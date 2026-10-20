import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { AFTER_LOGIN } from '../hosted/afterLogin';
import { HostedProvider } from '../hosted/HostedGate';
import { SignInPrompt } from './SignInPrompt';

afterEach(() => { sessionStorage.clear(); vi.restoreAllMocks(); });

it('signing in from a Community page comes back to that page', () => {
  render(<HostedProvider value={{ kind: 'signed-out' }}><MemoryRouter initialEntries={['/community/b/review-team?tab=jobs']}><SignInPrompt why="Sign in to save it." /></MemoryRouter></HostedProvider>);
  const link = screen.getByRole('link', { name: 'Sign in' });
  link.addEventListener('click', (e) => e.preventDefault());
  fireEvent.click(link);
  expect(sessionStorage.getItem(AFTER_LOGIN)).toBe('/community/b/review-team?tab=jobs');
});

it('on this computer, signing in shows a code to approve right here', async () => {
  const { fakeFetch } = await import('./testing');
  const { resetCommunityLink } = await import('./link');
  fakeFetch({
    'GET /api/community-link': () => [200, { enabled: true, origin: 'http://stub', signed_in: false, account: null }],
    'POST /api/community-link/pair': () => [200, { user_code: 'ABCD-EFGH', verify_url: 'http://stub/connect?code=ABCD-EFGH', expires_in: 600 }],
    'GET /api/community-link/pair': () => [200, { status: 'pending' }],
  });
  render(<MemoryRouter><SignInPrompt why="Sign in to save it." /></MemoryRouter>);
  expect(screen.getByText(/Sign in to save it\./)).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: 'Sign in' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Sign in to Sculptured' }));
  expect(await screen.findByText('ABCD-EFGH')).toBeInTheDocument();
  resetCommunityLink();
});
