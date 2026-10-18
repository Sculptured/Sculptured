import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import { STATES } from '../herdr';
import { seatView } from '../test-fixtures';
import type { SeatView } from '../generated/SeatView';
import { BroadcastDialog } from './Broadcast';
import type { AgentRow } from './model';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, send: vi.fn() } };
});

const row = (member: string, name: string, over: Partial<SeatView> = {}): AgentRow =>
  ({ id: `dev.${member}@r`, rig: 'r', name, group: 'Dev', folder: '~', seat: seatView(member, over), state: { ...STATES.idle, key: 'idle' }, vendor: { cls: 'v-practice', name: 'Practice' }, summary: 'idle', lastMs: null, activity: undefined }) as unknown as AgentRow;

const team = () => [row('a', 'Planner'), row('b', 'Builder'), row('c', 'Reviewer'), row('d', 'Archivist', { lifecycle: 'stopped' })];

beforeEach(() => vi.mocked(api.send).mockReset());

describe('Message several agents', () => {
  it('asks first, naming every recipient, then reports who got it and who did not', async () => {
    vi.mocked(api.send).mockImplementation(async (id) => {
      if (id === 'dev.b@r') throw new ApiError(409, 'seat_not_running', 'seat dev.b@r is not running');
      return { message_id: 'm' };
    });
    const onClose = vi.fn();
    render(<BroadcastDialog agents={team()} initial={[]} team="Web shop" onClose={onClose} />);
    const user = userEvent.setup();
    const dialog = screen.getByRole('dialog', { name: 'Message agents · Web shop' });
    // Every running agent is ticked; a stopped one cannot be, and says why.
    expect(within(dialog).getByRole('checkbox', { name: /Archivist/ })).toBeDisabled();
    expect(within(dialog).getByText('stopped; only running agents get messages')).toBeInTheDocument();
    expect(within(dialog).getByText('Type a message to send.')).toBeInTheDocument();
    await user.keyboard('Please write down what you have so far.');
    await user.click(within(dialog).getByRole('button', { name: 'Send to 3 agents…' }));
    expect(api.send).not.toHaveBeenCalled();
    const confirm = screen.getByRole('alertdialog', { name: 'Send to 3 agents?' });
    expect(confirm).toHaveAccessibleDescription('The same message goes to Planner, Builder and Reviewer. Each agent gets its own copy.');
    expect(within(confirm).getByRole('button', { name: 'Back' })).toHaveFocus();
    await user.click(within(confirm).getByRole('button', { name: 'Send to 3 agents' }));
    expect(api.send).toHaveBeenCalledTimes(3);
    expect(vi.mocked(api.send).mock.calls[0][1]).toEqual({ text: 'Please write down what you have so far.', intent: 'inform', interrupt: false });
    expect(await screen.findByText('Sent to 2 of 3. 1 agent did not get it.')).toBeInTheDocument();
    const results = screen.getByRole('list', { name: 'Who got the message' });
    expect(within(results).getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      '✓ Planner — sent',
      expect.stringMatching(/^▲ Builder — not sent: /),
      '✓ Reviewer — sent',
    ]);
    // Plain words, never the server's own text.
    expect(results.textContent).not.toContain('dev.b@r');
    // Send again goes only to the one that failed.
    vi.mocked(api.send).mockClear().mockResolvedValue({ message_id: 'm2' });
    await user.click(screen.getByRole('button', { name: 'Send again to 1 agent' }));
    expect(vi.mocked(api.send).mock.calls.map((c) => c[0])).toEqual(['dev.b@r']);
    expect(await screen.findByText('Sent to 3 agents.')).toBeInTheDocument();
  });

  it('keeps the request id after a network failure, so sending again delivers once', async () => {
    vi.mocked(api.send).mockRejectedValueOnce(new TypeError('Failed to fetch')).mockResolvedValue({ message_id: 'm' });
    render(<BroadcastDialog agents={team()} initial={['dev.a@r']} team="Web shop" onClose={() => {}} />);
    const user = userEvent.setup();
    await user.keyboard('hello');
    // One recipient: no confirmation.
    await user.click(screen.getByRole('button', { name: 'Send' }));
    expect(await screen.findByText('Sent to 0 of 1. 1 agent did not get it.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Send again to 1 agent' }));
    const [first, second] = vi.mocked(api.send).mock.calls;
    expect(second[2]).toBe(first[2]);
  });

  it('starts with the picked agents ticked, Esc steps back from the confirmation', async () => {
    render(<BroadcastDialog agents={team()} initial={['dev.a@r', 'dev.c@r']} team="Web shop" onClose={() => {}} />);
    const user = userEvent.setup();
    const ticked = screen.getAllByRole('checkbox').filter((c) => (c as HTMLInputElement).checked);
    expect(ticked.map((c) => c.closest('label')?.textContent)).toEqual([expect.stringContaining('Planner'), expect.stringContaining('Reviewer')]);
    await user.keyboard('hi{Control>}{Enter}{/Control}');
    expect(screen.getByRole('alertdialog', { name: 'Send to 2 agents?' })).toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(screen.getByRole('dialog', { name: 'Message agents · Web shop' })).toBeInTheDocument();
    expect(api.send).not.toHaveBeenCalled();
  });
});
