import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { stream, twoTeams, where } from '../shell/shellHarness';
import { seatView, snapshot } from '../test-fixtures';
import { OutcomeNote, outcomeOf } from './ActionButton';
import { stopQuestion } from './EmergencyStop';
import { A, control, header, layoutTab, pane, resetControl } from './pageHarness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  // The calls the Control page makes (see pageHarness).
  const fns = ['listRigs', 'latestSeatEvents', 'teamQuestions', 'answer', 'teamUsage', 'teamBudget', 'runUsage', 'teamControls', 'controlAgent', 'agentControl',
    'pauseTeam', 'resumeTeam', 'stopTeamNow', 'send', 'machineMetrics', 'runCommunications', 'managedChildren', 'teamRecent', 'deployments', 'seatQuestions'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(fns.map((f) => [f, vi.fn()])) } };
});

beforeEach(resetControl);
afterEach(() => vi.unstubAllGlobals());

const B = 'dev.b@r';
const stopped = () => snapshot({ seats: [seatView('a', { lifecycle: 'stopped' }), seatView('b', { lifecycle: 'stopped' })] });

describe('Control header', () => {
  it('pause_all_lists_skipped_agents_and_notes', async () => {
    vi.mocked(api.pauseTeam).mockResolvedValue({
      affected: [A],
      skipped: [{ seat: B, code: 'something_new' as never, reason: 'the server said no' }],
      notes: [{ seat: A, reason: 'kept its session' }],
    } as never);
    await control();
    await userEvent.setup().click(within(header()).getByRole('button', { name: 'Pause all' }));
    const status = await screen.findByText('Paused 1 agent; 1 agent left as they were.');
    const lines = within(status.closest('.ctl-outcome') as HTMLElement).getByRole('list', { name: 'Each agent' });
    expect(within(lines).getAllByRole('listitem').map((l) => l.textContent)).toEqual(['b: the server said no', 'a: kept its session']);
    // The server's own words are evidence.
    expect(within(lines).getByText('the server said no')).toHaveAttribute('data-raw-evidence');
    expect(within(lines).getByText('kept its session')).toHaveAttribute('data-raw-evidence');
  });

  it('refused_is_not_success', () => {
    const o = outcomeOf({ text: 'Fix 2 things first', skipped: [], refused: true });
    render(<OutcomeNote o={o} dismiss={() => {}} />);
    const note = screen.getByRole('status');
    expect(note).toHaveTextContent('◇ Fix 2 things first');
    expect(note).toHaveClass('tone-warn');
  });

  it('start_says_running_in_one_line_while_running', async () => {
    await control();
    const start = within(header()).getByRole('button', { name: 'Start' });
    expect(start).toBeDisabled();
    expect(start.parentElement!.querySelector('.ctl-why')!.firstChild!.textContent).toBe('Running');
    expect(start).toHaveAccessibleDescription(/The team is already running/);
  });

  it('emergency_stop_never_reopens_by_itself_and_closes_outside', async () => {
    await control();
    const user = userEvent.setup();
    const stop = () => within(header()).getByRole('button', { name: 'Emergency stop' });
    await user.click(stop());
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Stop all 2 agents now?');
    act(() => stream().emit('snapshot', stopped()));
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull());
    act(() => stream().emit('snapshot', snapshot()));
    await waitFor(() => expect(stop()).toBeEnabled());
    expect(screen.queryByRole('alertdialog')).toBeNull();
    // Opened again: focus stays inside; a press outside closes it.
    await user.click(stop());
    const ask = screen.getByRole('alertdialog');
    expect(within(ask).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await user.tab();
    expect(within(ask).getByRole('button', { name: 'Stop now' })).toHaveFocus();
    await user.tab({ shift: true });
    expect(within(ask).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(api.stopTeamNow).not.toHaveBeenCalled();
  });

  it('one agent: the question names it as one', () => {
    expect(stopQuestion(1)).toBe('Stop the agent now?');
    expect(stopQuestion(3)).toBe('Stop all 3 agents now?');
  });

  it('the project bar leaves status and Stop team to the header on Control only', async () => {
    await twoTeams('/p/r/control');
    const bar = () => document.querySelector<HTMLElement>('.project-bar')!;
    expect(within(bar()).queryByRole('button', { name: /^(Start team|Stop team…)$/ })).toBeNull();
    expect(bar().querySelector('.pstatus')).toBeNull();
    await userEvent.setup().click(within(screen.getByRole('navigation', { name: 'Project' })).getByRole('link', { name: 'Agents' }));
    expect(within(bar()).getByRole('button', { name: /^(Start team|Stop team…)$/ })).toBeInTheDocument();
  });
});

describe('Agent detail', () => {
  it('keeps its own Activity view and follows the selection only while Follow is on', async () => {
    await control();
    const user = userEvent.setup();
    await user.click(layoutTab('Agent focus'));
    await user.click(within(pane('Agent detail')).getByRole('button', { name: /^a / }));
    const detail = () => pane('Agent detail');
    await user.click(within(detail()).getByRole('button', { name: 'Activity' }));
    await user.click(within(detail()).getByRole('button', { name: /Raw events/ }));
    expect(within(detail()).getByRole('table', { name: 'Raw events' })).toBeInTheDocument();
    expect(where()).toBe('/p/r/control');
    // Follow off: a new selection leaves this pane on a.
    await user.click(within(detail()).getByRole('checkbox', { name: 'Follow selection' }));
    const lane = document.querySelector<HTMLElement>(`[data-lane="${B}"] .ctl-track`)!;
    act(() => lane.focus());
    await user.keyboard('{End}');
    expect(document.querySelector(`[data-lane="${B}"]`)).toHaveClass('is-picked');
    expect(within(detail()).getByRole('heading', { name: 'a' })).toBeInTheDocument();
    await user.click(within(detail()).getByRole('checkbox', { name: 'Follow selection' }));
    expect(within(detail()).getByRole('heading', { name: 'b' })).toBeInTheDocument();
  });
});
