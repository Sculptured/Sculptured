import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { CommunityCard } from './CommunityCard';
import { resetCommunityLink } from './link';
import { fakeFetch } from './testing';

afterEach(() => { vi.restoreAllMocks(); resetCommunityLink(); });

it('says a sign-in may remain on the server, where to remove it, until dismissed', async () => {
  let left = true;
  const { calls } = fakeFetch({
    'GET /api/community-link': () => [200, { enabled: false, origin: null, signed_in: false, account: null, ...(left ? { key_left_on: 'https://app.usesculptured.com' } : {}) }],
    'DELETE /api/community-link/key-left': () => { left = false; return [200, { enabled: false, origin: null, signed_in: false, account: null }]; },
  });
  render(<MemoryRouter><CommunityCard /></MemoryRouter>);
  expect(await screen.findByText(/could not remove this computer’s Community sign-in from app\.usesculptured\.com/)).toBeVisible();
  expect(screen.getByRole('link', { name: 'Computers page' })).toHaveAttribute('href', 'https://app.usesculptured.com/computers');
  await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
  expect(calls).toContain('DELETE /api/community-link/key-left');
  await vi.waitFor(() => expect(screen.queryByText(/could not remove/)).toBeNull());
});
