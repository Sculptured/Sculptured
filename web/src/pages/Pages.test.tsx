import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { resetSeenCache } from '../seen';
import { forgetViewports } from '../shell/LiveGraph';
import { stream, twoTeams, where } from '../shell/shellHarness';
import { ev, FakeEventSource, seatView, snapshot } from '../test-fixtures';
import { openRigWords } from '../test-vocab';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn(), teamRecent: vi.fn(), listRigs: vi.fn(), send: vi.fn(), gitStatus: vi.fn(), deployments: vi.fn() } };
});

beforeEach(() => {
  FakeEventSource.reset();
  forgetViewports();
  localStorage.clear();
  resetSeenCache();
  vi.mocked(api.latestSeatEvents).mockReset().mockResolvedValue([]);
  vi.mocked(api.teamRecent).mockReset().mockResolvedValue({});
  vi.mocked(api.listRigs).mockReset().mockResolvedValue([]);
  vi.mocked(api.send).mockReset().mockResolvedValue({ message_id: 'm1' });
  vi.mocked(api.gitStatus).mockReset().mockRejectedValue(new Error('no folder'));
  vi.mocked(api.deployments).mockReset().mockResolvedValue([]);
});

describe('Agents list', () => {
  it('a row opens its agent; ticked rows are messaged together, and Message all ticks every running one', async () => {
    await twoTeams('/p/r/agents');
    const table = await screen.findByRole('table');
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows.map((r) => r.querySelector('a')?.textContent)).toEqual(['a', 'b']);
    for (const head of ['Name', 'Runs on', 'Status', 'Folder', 'Last activity', 'Last message']) expect(within(table).getByRole('columnheader', { name: head })).toBeInTheDocument();
    const user = userEvent.setup();
    expect(screen.getByRole('button', { name: 'Message selected…' })).toBeDisabled();
    await user.click(within(rows[1]).getByRole('checkbox', { name: 'Select b' }));
    await user.click(screen.getByRole('button', { name: 'Message selected…' }));
    const dialog = screen.getByRole('dialog', { name: /^Message agents/ });
    expect(within(dialog).getAllByRole('checkbox').map((c) => (c as HTMLInputElement).checked)).toEqual([false, true]);
    await user.click(within(dialog).getByRole('button', { name: 'Close' }));
    await user.click(within(rows[0]).getByRole('link', { name: /a/ }));
    expect(where()).toBe('/p/r/agents/dev.a');
    expect(screen.getByRole('link', { name: 'Conversation' })).toHaveAttribute('aria-current', 'page');
  });

  it('says what each agent last said, as agent-reported evidence', async () => {
    vi.mocked(api.teamRecent).mockResolvedValue({ 'dev.a@r': [ev(4, { type: 'agent-text', turn_id: 't1', text: 'I fixed the parser' })] });
    await twoTeams('/p/r/agents');
    const said = await screen.findByText('I fixed the parser');
    expect(said).toHaveAttribute('data-raw-evidence');
    expect(said.closest('.said')!.querySelector('.ux-prov-mark')).toHaveAttribute('data-tip', expect.stringContaining('agent-reported'));
  });
});

describe('Results', () => {
  const finished = () => snapshot({ seats: [seatView('a'), seatView('b')] });

  it('says honestly what each agent reported, files unknown when unreadable, and that nothing is verified', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await twoTeams('/p/r/results', finished());
    for (const id of ['dev.a@r', 'dev.b@r'])
      act(() => stream().emit('event', { event: { ...ev(9, { type: 'turn-ended', turn_id: 't', status: 'completed' }), subject: { kind: 'seat', id } }, seat: seatView(id === 'dev.a@r' ? 'a' : 'b') }));
    await act(async () => { vi.advanceTimersByTime(2000); });
    const summary = await screen.findByRole('region', { name: 'Run finished' });
    expect(summary).toHaveTextContent('Every agent finished its turn; the team is still running.');
    expect(within(screen.getByRole('region', { name: 'Files changed' })).getByText('unknown')).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Checks' })).toHaveTextContent('Verified: nothing.');
    expect(screen.getByRole('region', { name: 'Usage this run' })).toBeInTheDocument();
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
    vi.useRealTimers();
  });

  it('has its own tabs for the team conversation, usage, communications, knowledge and history', async () => {
    await twoTeams('/p/r/results', finished());
    const tabs = screen.getByRole('navigation', { name: 'Results' });
    expect(within(tabs).getAllByRole('link').map((l) => l.textContent)).toEqual(['Summary', 'Team conversation', 'Usage', 'Communications', 'Knowledge', 'History']);
    await userEvent.setup().click(within(tabs).getByRole('link', { name: 'History' }));
    expect(where()).toBe('/p/r/results/history');
    await waitFor(() => expect(api.deployments).toHaveBeenCalled());
    // The tree's cards are placed absolutely, so History is the page's column that gives it its height.
    expect(screen.getByRole('main')).toHaveClass('results-history');
    expect(document.querySelector('.results-history > .deployment-history')).not.toBeNull();
  });

  it('before a first start, says there is nothing yet', async () => {
    await twoTeams('/p/none/results');
    expect(await screen.findByRole('heading', { name: 'No runs yet' })).toBeInTheDocument();
  });
});
