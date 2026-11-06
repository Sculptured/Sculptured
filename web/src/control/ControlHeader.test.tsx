import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { stream, twoTeams, where } from '../shell/shellHarness';
import { seatView, snapshot } from '../test-fixtures';
import { MemoryRouter } from 'react-router-dom';
import { WORDS } from '../vocab';
import { OutcomeNote, outcomeOf } from './ActionButton';
import type { ControlCtx } from './context';
import { ControlHeader } from './ControlHeader';
import type { Action, ControlSource } from './source';
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

describe('Control header, round 2', () => {
  it('stop_team_asks_the_project_bar_for_the_graceful_stop', async () => {
    await control();
    await userEvent.setup().click(within(header()).getByRole('button', { name: 'Stop team…' }));
    expect(screen.getByRole('group', { name: WORDS.confirmStopTeam })).toBeInTheDocument();
    expect(api.stopTeamNow).not.toHaveBeenCalled();
    // Nothing running: disabled, with its reason.
    act(() => stream().emit('snapshot', stopped()));
    await waitFor(() => expect(within(header()).getByRole('button', { name: 'Stop team…' })).toBeDisabled());
    expect(within(header()).getByRole('button', { name: 'Stop team…' })).toHaveAccessibleDescription('Nothing is running');
  });

  it('emergency_confirm_stays_while_its_stop_is_sent_then_focus_goes_to_what_it_did', async () => {
    let finish: (v: { stopped: number }) => void = () => {};
    vi.mocked(api.stopTeamNow).mockImplementation(() => new Promise((r) => { finish = r; }));
    await control();
    const user = userEvent.setup();
    await user.click(within(header()).getByRole('button', { name: 'Emergency stop' }));
    await user.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Stop now' }));
    const ask = screen.getByRole('alertdialog');
    expect(ask).toHaveTextContent('Stop all 2 agents now?');
    expect(within(ask).getByRole('button', { name: 'Stopping…' })).toBeDisabled();
    await act(async () => finish({ stopped: 2 }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    await waitFor(() => expect(screen.getByText(/Stopping 2 agents now/).closest('.ctl-outcome')).toHaveFocus());
  });

  it('a start refused for the design links to where to fix it', () => {
    const off: Action = { can: { ok: false, why: 'Fix 2 things first' }, run: async () => ({ text: 'Fix 2 things first', skipped: [], refused: true }) };
    const source = { kind: 'deployment', title: 't', running: false, agents: [], feed: [], approvals: [], metrics: { toolCallsPerMin: [] },
      team: { start: off, stop: off, pauseAll: off, resumeAll: off, emergencyStop: off } } as unknown as ControlSource;
    const alarms = { list: [], acked: new Set<string>(), unacked: 0, ack() {}, ackAll() {}, shelved: new Map<string, number>(), shelve() {}, unshelve() {}, rules: [], setRules() {} };
    const ctx: ControlCtx = { kind: 'deployment', team: 'r', source, alarms, show() {}, now: 0, startFix: { text: 'Fix 2 things', to: '/p/r/board?mode=design' } };
    render(<MemoryRouter><ControlHeader ctx={ctx} /></MemoryRouter>);
    expect(screen.getByRole('link', { name: 'Fix 2 things' })).toHaveAttribute('href', '/p/r/board?mode=design');
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
    // One follow control: its Settings pick an agent, without a second Follow selection switch.
    await user.click(within(detail()).getByRole('button', { name: 'Agent detail options' }));
    await user.click(screen.getByRole('menuitem', { name: /Settings/ }));
    const form = screen.getByRole('group', { name: 'Agent detail settings' });
    expect(within(form).queryByRole('checkbox', { name: 'Follow selection' })).toBeNull();
    expect(within(form).getByRole('combobox', { name: 'Agent' })).toBeInTheDocument();
  });
});
