import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import { ComponentsSection } from '../structure/ComponentsSection';
import { teamDraft } from '../test-fixtures';
import { resetCommunityLink } from './link';
import { ShareSignIn } from './ShareSignIn';
import { fakeFetch } from './testing';

afterEach(() => { vi.restoreAllMocks(); resetCommunityLink(); });

const off = { enabled: false, origin: null, signed_in: false, account: null };
const on = { enabled: true, origin: 'http://stub', signed_in: false, account: null };

it('this computer, Community off: sharing points to the Turn on page and asks Community nothing', async () => {
  const { calls } = fakeFetch({ 'GET /api/community-link': () => [200, off] });
  render(<ShareSignIn />);
  expect(await screen.findByRole('link', { name: 'Turn on Community to share' })).toHaveAttribute('href', '/community');
  expect(calls.filter((c) => c.includes('/api/community/'))).toEqual([]);
});

it('this computer, on but signed out: sharing shows the code to approve right there', async () => {
  const { calls } = fakeFetch({
    'GET /api/community-link': () => [200, on],
    'POST /api/community-link/pair': () => [200, { user_code: 'ABCD-EFGH', verify_url: 'http://stub/connect?code=ABCD-EFGH', expires_in: 600 }],
    'GET /api/community-link/pair': () => [200, { status: 'pending' }],
  });
  render(<ShareSignIn />);
  fireEvent.click(await screen.findByRole('button', { name: 'Sign in to share' }));
  expect(await screen.findByText('ABCD-EFGH')).toBeInTheDocument();
  expect(calls).toContain('POST /api/community-link/pair');
  expect(calls.some((c) => c.includes('/api/community/login'))).toBe(false);
});

it('hosted: sharing signs in through the sign-in page', () => {
  render(<HostedProvider value={{ kind: 'signed-out' }}><ShareSignIn /></HostedProvider>);
  expect(screen.getByRole('link', { name: 'Sign in to share' })).toHaveAttribute('href', '/login');
});

it('a team component offers this way to sign in, never the retired Community sign-in address', async () => {
  fakeFetch({ 'GET /api/community-link': () => [200, off] });
  const notes = { id: 'notes', name: 'Notes server', description: 'Team notes.', kind: 'mcp', program: 'notes-mcp', install: 'pip install notes-mcp' };
  const d = teamDraft({ id: 't', name: 'T', components: [notes] } as never);
  const { container } = render(<ComponentsSection draft={d} edit={() => null} locked={false} />);
  expect(await screen.findByRole('link', { name: 'Turn on Community to share' })).toBeInTheDocument();
  expect(container.querySelector('a[href="/api/community/login"]')).toBeNull();
});
