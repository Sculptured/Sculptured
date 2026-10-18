import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { ev, snapshot } from '../test-fixtures';
import type { EventRecord } from '../generated/EventRecord';
import { TeamConversation } from './TeamConversation';
import { emptyState } from '../useRigStream';

vi.mock('../api', async orig => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn() } };
});
const reply = (seq: number, text: string, seat = 'dev.a@r') => ev(seq, { type: 'agent-text', turn_id: 't', text }, seat);
const snap = snapshot();
const state = { ...emptyState, snapshot: snap, seats: Object.fromEntries(snap.seats.map(s => [s.id, s])) };
const props = (feed: EventRecord[] = []) => ({ rig: 'r', state: { ...state, feed: { 'dev.a@r': feed } }, connected: true, names: { 'dev.a@r': 'Builder', 'dev.b@r': 'Reviewer' }, agents: {}, onOpen: vi.fn(), onAnswer: vi.fn(), onClose: vi.fn() });

beforeEach(() => { vi.mocked(api.latestSeatEvents).mockReset().mockResolvedValue([]); });

describe('board team conversation', () => {
  it('filters both sides of handoffs, preserves filters across inspection, and opens precise evidence', async () => {
    const user = userEvent.setup();
    const feed = [reply(1, 'Intermediate result'),
      ev(2, { type: 'message-queued', message_id: 'm', intent: 'inform', text: 'Review this draft' }, 'dev.b@r'),
      ev(3, { type: 'handed-over', start_seq: 1, message_id: 'm', from: [{ seat: 'dev.a@r', turn_id: 't', turn_seq: 1, requested_seq: null }], resend_of: null, wave: null }, 'dev.b@r')];
    const p = props(feed);
    const { unmount } = render(<MemoryRouter><TeamConversation {...p} /></MemoryRouter>);
    await waitFor(() => expect(api.latestSeatEvents).toHaveBeenCalledTimes(2));
    await user.selectOptions(screen.getByLabelText('Agent'), 'dev.a@r');
    await user.selectOptions(screen.getByLabelText('Show'), 'exchanges');
    expect(screen.queryByText('Intermediate result')).toBeNull();
    const list = within(screen.getByRole('list', { name: 'Team exchanges' }));
    expect(list.getByText('Review this draft')).toBeVisible();
    expect(list.getByRole('link', { name: 'Open conversation' })).toHaveAttribute('href', '/p/r/agents/dev.b/conversation?item=m-m');
    await user.click(list.getByRole('button', { name: 'Reviewer' }));
    expect(p.onOpen).toHaveBeenCalledWith('dev.b@r');
    unmount();
    render(<MemoryRouter><TeamConversation {...p} /></MemoryRouter>);
    expect(screen.getByLabelText('Agent')).toHaveValue('dev.a@r');
    expect(screen.getByLabelText('Show')).toHaveValue('exchanges');
    await user.selectOptions(screen.getByLabelText('Agent'), '');
    await user.selectOptions(screen.getByLabelText('Show'), '');
  });
  it('freezes updates while reading, resumes, and displays gaps and history failures', async () => {
    vi.mocked(api.latestSeatEvents).mockRejectedValue(new Error('offline'));
    const user = userEvent.setup();
    const p = { ...props([reply(1, 'Initial observation')]), rig: 'pause-test' };
    const { rerender } = render(<MemoryRouter><TeamConversation {...p} connected={false} state={{ ...p.state, gaps: 3 }} /></MemoryRouter>);
    await screen.findByRole('alert');
    expect(screen.getByText(/3 events lost/)).toBeVisible();
    expect(screen.getByText(/Reconnecting/)).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Pause updates' }));
    rerender(<MemoryRouter><TeamConversation {...p} state={props([reply(1, 'Initial observation'), reply(2, 'Second observation')]).state} /></MemoryRouter>);
    expect(screen.queryByText('Second observation')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Resume updates (1 new)' }));
    expect(screen.getByText('Second observation')).toBeVisible();
    await user.keyboard('{Escape}');
    expect(p.onClose).toHaveBeenCalledOnce();
  });

});
