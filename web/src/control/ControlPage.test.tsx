import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { renderApp, stream, twoTeams, where } from '../shell/shellHarness';
import { seatView, snapshot } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { saveLayouts } from '../workspace/store';
import { controlPage } from './ControlPage';
import { controlDefaults } from './defaults';
import { A, control, header, layoutTab, pane, paneTab, resetControl } from './pageHarness';
import { question } from './testHarness';
import { ackKey } from './useAlarms';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  // The calls the Control page makes (CONTROL_API in pageHarness; vi.mock is hoisted, so listed here).
  const fns = ['listRigs', 'latestSeatEvents', 'teamQuestions', 'answer', 'teamUsage', 'teamBudget', 'runUsage', 'teamControls', 'controlAgent', 'agentControl',
    'pauseTeam', 'resumeTeam', 'stopTeamNow', 'send', 'machineMetrics', 'runCommunications', 'managedChildren', 'teamRecent', 'deployments', 'seatQuestions'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(fns.map((f) => [f, vi.fn()])) } };
});

beforeEach(resetControl);
afterEach(() => vi.unstubAllGlobals());

describe('Control page', () => {
  it('default_live_layout_shows_board_first', async () => {
    await control();
    expect(layoutTab('Live')).toHaveAttribute('aria-selected', 'true');
    const tabs = [...document.querySelectorAll('[data-tab]')].map((t) => t.textContent);
    expect(tabs[0]).toContain('System overview');
    expect(within(pane('System overview')).getByRole('region', { name: 'Run board' })).toBeInTheDocument();
    for (const t of ['Status', 'Timeline']) expect(pane(t)).toBeInTheDocument();
    expect(within(screen.getByRole('navigation', { name: 'Project' })).getAllByRole('link')[0]).toHaveTextContent('Control');
    expect(screen.getByRole('list', { name: 'Run status' })).toHaveTextContent('running');
  });

  it('selecting_agent_in_overview_updates_agent_detail_and_timeline', async () => {
    await control();
    const card = within(pane('System overview')).getByRole('button', { name: 'a — idle' });
    act(() => card.focus());
    await waitFor(() => expect(document.querySelector(`[data-lane="${A}"]`)).toHaveClass('is-picked'));
    expect(document.querySelector('[data-lane="dev.b@r"]')).not.toHaveClass('is-picked');
    await userEvent.setup().keyboard('{Enter}');
    expect(where()).toBe('/p/r/control');
    const detail = await screen.findByRole('region', { name: 'Agent detail' });
    expect(within(detail).getByRole('heading', { name: 'a' })).toBeInTheDocument();
    await userEvent.setup().click(layoutTab('Agent focus'));
    expect(within(pane('Agent detail')).getByRole('heading', { name: 'a' })).toBeInTheDocument();
  });

  it('approve_and_reject_with_reason', async () => {
    vi.mocked(api.teamQuestions).mockResolvedValue([
      question('q1'),
      question('q2', { source: 'native-permission', prompt: { header: null, text: 'rm -rf build', options: [], allow_other: false, tool: 'Bash', tool_input: null } }),
      question('q3', { source: 'request-input' }),
    ]);
    vi.mocked(api.answer).mockResolvedValue(undefined as never);
    await control();
    const user = userEvent.setup();
    await user.click(paneTab('Approvals'));
    const list = await screen.findByRole('list', { name: 'Waiting for you' });
    const [change, tool, ask] = within(list).getAllByRole('listitem');
    await user.click(within(change).getByRole('button', { name: 'Approve' }));
    expect(api.answer).toHaveBeenCalledWith('q1', { kind: 'allow' }, expect.any(String));
    await user.click(within(tool).getByRole('button', { name: 'Reject…' }));
    const submit = within(tool).getByRole('button', { name: 'Reject' });
    expect(submit).toBeDisabled();
    expect(tool).toHaveTextContent('Type a reason to reject.');
    await user.type(within(tool).getByRole('textbox', { name: /Reason/ }), 'Too risky');
    await user.click(submit);
    expect(api.answer).toHaveBeenCalledWith('q2', { kind: 'deny', reason: 'Too risky' }, expect.any(String));
    expect(within(ask).queryByRole('button', { name: 'Approve' })).toBeNull();
    expect(within(ask).getByRole('link', { name: 'Answer in Needs you' })).toHaveAttribute('href', '/p/r/questions');
  });

  it('emergency_stop_confirm_and_idle', async () => {
    let finish: (v: { stopped: number }) => void = () => {};
    vi.mocked(api.stopTeamNow).mockImplementation(() => new Promise((r) => { finish = r; }));
    await control();
    const user = userEvent.setup();
    const stop = within(header()).getByRole('button', { name: 'Emergency stop' });
    await user.click(stop);
    const ask = screen.getByRole('alertdialog', { name: 'Emergency stop' });
    expect(ask).toHaveTextContent('Stop all 2 agents now?');
    await user.click(within(ask).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(api.stopTeamNow).not.toHaveBeenCalled();
    await user.click(stop);
    await user.dblClick(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Stop now' }));
    await act(async () => finish({ stopped: 2 }));
    expect(api.stopTeamNow).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/Stopping 2 agents now/)).toBeInTheDocument();
    // Idle: nothing runs, so it is disabled and says why.
    const idle = snapshot({ seats: [seatView('a', { lifecycle: 'stopped' }), seatView('b', { lifecycle: 'stopped' })] });
    act(() => stream().emit('snapshot', idle));
    await waitFor(() => expect(within(header()).getByRole('button', { name: 'Emergency stop' })).toBeDisabled());
    expect(within(header()).getByRole('button', { name: 'Emergency stop' })).toHaveAccessibleDescription('Nothing is running');
    expect(header()).toHaveTextContent('Nothing is running');
  });

  it('agent_detail_has_one_stop_and_it_asks_first', async () => {
    vi.mocked(api.controlAgent).mockResolvedValue({});
    await control();
    const user = userEvent.setup();
    const card = within(pane('System overview')).getByRole('button', { name: 'a — idle' });
    act(() => card.focus());
    await user.keyboard('{Enter}');
    await user.click(layoutTab('Agent focus'));
    const detail = pane('Agent detail');
    await user.click(within(detail).getByRole('button', { name: 'Activity' }));
    // One set of controls: the pane's own; the activity view adds no second Stop.
    expect(within(detail).queryByRole('button', { name: 'Hard stop…' })).toBeNull();
    expect(within(detail).getAllByRole('button', { name: 'Stop' })).toHaveLength(1);
    await user.click(within(detail).getByRole('button', { name: 'Stop' }));
    const ask = screen.getByRole('alertdialog', { name: 'Stop' });
    expect(ask).toHaveTextContent('Stop a now?');
    expect(within(ask).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(api.controlAgent).not.toHaveBeenCalled();
    await user.click(within(detail).getByRole('button', { name: 'Stop' }));
    await user.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Stop now' }));
    await waitFor(() => expect(api.controlAgent).toHaveBeenCalledWith(A, 'hard', expect.any(String)));
    expect(await within(detail).findByText(/Stopping a\./)).toBeInTheDocument();
    // The pane reads the agent's mode from the team's controls, not a poll of its own.
    expect(api.agentControl).not.toHaveBeenCalled();
  });

  it('alarm_ack_persists', async () => {
    const failed = snapshot({ seats: [seatView('a', { lifecycle: 'failed' }), seatView('b')] });
    const first = await twoTeams('/p/r/control', failed).then(() => screen);
    const caution = await first.findByRole('button', { name: /^Master caution: 1 not acknowledged/ });
    const user = userEvent.setup();
    await user.click(caution);
    expect(paneTab('Alarms')).toHaveAttribute('aria-selected', 'true');
    const list = screen.getByRole('list', { name: 'Alarms, newest first' });
    expect(list).toHaveTextContent('▲ new');
    await user.click(within(list).getByRole('button', { name: 'Acknowledge' }));
    expect(screen.getByRole('button', { name: /^Master caution: 1 acknowledged/ })).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem(ackKey('r'))!)).toHaveLength(1);
    expect(list).toHaveTextContent('✓ acknowledged');
    // Kept: a fresh page still has it acknowledged.
    document.body.innerHTML = '';
    renderApp('/p/r/control');
    await act(async () => {});
    act(() => stream().emit('snapshot', failed));
    expect(await screen.findByRole('button', { name: /^Master caution: 1 acknowledged/ })).toBeInTheDocument();
  });

  it('pane_route_renders_single_pane', async () => {
    saveLayouts(controlPage('r'), controlDefaults('deployment'));
    await twoTeams('/p/r/control/pane/live-status');
    await act(async () => {});
    expect(screen.getByRole('heading', { level: 1, name: 'Status' })).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Status feed, newest first' })).toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Timeline' })).toBeNull();
    expect(screen.queryByRole('tablist', { name: 'Layouts' })).toBeNull();
    expect(document.querySelector('.control-popout')).not.toBeNull();
  });

  it('no_openrig_words', async () => {
    vi.mocked(api.teamQuestions).mockResolvedValue([question('q1')]);
    await control(snapshot({ seats: [seatView('a', { lifecycle: 'failed' }), seatView('b')] }));
    const user = userEvent.setup();
    const app = document.querySelector<HTMLElement>('.app')!;
    expect(openRigWords(app)).toEqual([]);
    for (const t of ['Approvals', 'Alarms', 'Raw events']) {
      await user.click(paneTab(t));
      expect(openRigWords(app)).toEqual([]);
    }
    for (const l of ['Agent focus', 'Cost']) {
      await user.click(layoutTab(l));
      expect(openRigWords(app)).toEqual([]);
    }
  });
});
