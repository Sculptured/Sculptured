import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { AppRoutes } from '../App';
import type { EventRecord } from '../generated/EventRecord';
import { compiledRig, ev, FakeEventSource, openFake, seatView, snapshot } from '../test-fixtures';
import { LiveGraph, PULSE_MS, pulseKey } from './LiveGraph';
import { teamAgents } from './model';
import { forgetViewports } from './viewport';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return {
    ...m,
    api: {
      ...m.api,
      latestSeatEvents: vi.fn(),
      listRigs: vi.fn().mockResolvedValue([]),
      restartSeat: vi.fn().mockResolvedValue({}),
      handOver: vi.fn().mockResolvedValue({}),
      resendHandoffs: vi.fn().mockResolvedValue({ resent: 1 }),
    },
  };
});

function renderAt(url: string) {
  render(
    <MemoryRouter initialEntries={[url]}>
      <AppRoutes openStream={openFake} />
    </MemoryRouter>,
  );
}

/** b waits for a: the server's waiting record, as the seat view carries it. */
const waitingSnapshot = () =>
  snapshot({ seats: [seatView('a', { label: 'Planner' }), seatView('b', { label: 'Builder', waiting_for: ['dev.a@r'] })] });

function handedOver(seq: number): EventRecord {
  return {
    ...ev(seq, { type: 'handed-over', start_seq: 1, message_id: `h${seq}`, from: [{ seat: 'dev.a@r', turn_id: 'turn-1', turn_seq: seq - 1, requested_seq: null }], resend_of: null, wave: null }, 'dev.b@r'),
    actor: { kind: 'system', id: 'handoff' },
  };
}

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem('sculptured.panels.v2.left', 'true');
  FakeEventSource.reset();
  forgetViewports();
  vi.mocked(api.latestSeatEvents).mockResolvedValue([]);
  // These tests exercise the Run graph; Run opens on List unless the graph was chosen.
  localStorage.setItem('sculptured.run.view.v1', JSON.stringify('graph'));
});
afterEach(() => vi.useRealTimers());

describe('Handoffs: waiting state', () => {
  it('names the agents a seat waits for, by their names', () => {
    const rows = teamAgents('r', compiledRig(), { 'dev.b@r': seatView('b', { waiting_for: ['dev.a@r'] }) }, {}, { 'dev.a@r': 'Planner' });
    const b = rows.find((r) => r.id === 'dev.b@r')!;
    expect(b.waitingFor).toEqual(['Planner']);
    expect(b.summary).toBe('waiting for Planner');
    expect(rows.find((r) => r.id === 'dev.a@r')!.waitingFor).toEqual([]);
  });

  it('shows "◷ waiting for …" on the Run board card and in the Agents list', async () => {
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', waitingSnapshot()));
    const box = await screen.findByRole('button', { name: 'Builder — idle, waiting for Planner' });
    expect(box).toHaveTextContent('◷waiting for Planner');
    // Not waiting: the ordinary state only.
    expect(screen.getByRole('button', { name: 'Planner — idle' })).not.toHaveTextContent('waiting');
    await userEvent.click(screen.getByRole('link', { name: 'Agents' }));
    const row = (await screen.findByRole('link', { name: /Builder/ })).closest('tr')!;
    expect(row).toHaveTextContent('◷waiting for Planner');
  });

  it('shows the waiting state in the agent pane header, and drops it when the handoff arrives', async () => {
    renderAt('/rig/r/agent/dev.b%40r');
    act(() => FakeEventSource.instances[0].emit('snapshot', waitingSnapshot()));
    const pane = await screen.findByRole('region', { name: 'Agent Builder' });
    const head = pane.querySelector('.ap-notices')!;
    expect(head).toHaveTextContent("◷waiting for Planner — starts when Planner finishes; Planner's result is passed along");
    act(() => FakeEventSource.instances[0].emit('event', { event: handedOver(20), seat: seatView('b', { label: 'Builder' }) }));
    expect(head).not.toHaveTextContent('waiting for');
  });
});

describe('Handoffs: edge pulse', () => {
  it('pulses the edge a handoff travelled along, briefly, and announces it', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', waitingSnapshot()));
    expect(screen.queryByTestId('edge-dev.a@r-dev.b@r')).toBeNull();
    act(() => FakeEventSource.instances[0].emit('event', { event: handedOver(20), seat: seatView('b', { label: 'Builder' }) }));
    const edge = await screen.findByTestId('edge-dev.a@r-dev.b@r');
    expect(edge).toHaveClass('handoff-pulse');
    expect(edge).toHaveAttribute('data-pulse', 'handoff');
    expect(screen.getByTestId('handoff-announce')).toHaveTextContent('Planner handed work to Builder.');
    // The design-intent edge back to a never pulses.
    expect(screen.queryByTestId('edge-dev.b@r-dev.a@r')).toBeNull();
    act(() => {
      vi.advanceTimersByTime(PULSE_MS + 10);
    });
    expect(edge).not.toHaveClass('handoff-pulse');
    expect(screen.getByTestId('handoff-announce')).toHaveTextContent('');
  });

  it('draws only "hands work to" edges as pulsing', () => {
    const agents = Object.fromEntries(teamAgents('r', compiledRig(), { 'dev.a@r': seatView('a'), 'dev.b@r': seatView('b') }).map((a) => [a.id, a]));
    render(
      <LiveGraph
        rig="r"
        compiled={compiledRig()}
        agents={agents}
        onOpen={() => {}}
        onAnswer={() => {}}
        now={0}
        pulses={new Set([pulseKey('dev.a@r', 'dev.b@r'), pulseKey('dev.b@r', 'dev.a@r')])}
      />,
    );
    expect(screen.getByTestId('edge-dev.a@r-dev.b@r')).toHaveClass('handoff-pulse');
    expect(screen.getByTestId('edge-dev.b@r-dev.a@r')).not.toHaveClass('handoff-pulse');
  });
});

/** b is stuck: a needs attention, so b waits with no way forward by itself. */
const stuckSnapshot = () =>
  snapshot({
    seats: [
      seatView('a', { label: 'Planner', lifecycle: 'attention', lifecycle_reason: 'launch failed' }),
      seatView('b', { label: 'Builder', waiting_for: ['dev.a@r'], handoff_stuck: { reason: 'Planner needs attention', can_resend: false } }),
    ],
  });

describe('Handoffs: stuck state', () => {
  it('shows "▲ stuck: …" in red on the card and in the Agents list', async () => {
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', stuckSnapshot()));
    const box = await screen.findByRole('button', { name: 'Builder — stuck, stuck: Planner needs attention' });
    expect(box).toHaveTextContent('▲stuck');
    expect(box.querySelector('.stuck-mark')).toHaveTextContent('▲stuck: Planner needs attention');
    expect(box.querySelector('.stuck-mark')).toHaveClass('tone-red');
    expect(box.querySelector('.waiting-mark')).toBeNull();
    await userEvent.click(screen.getByRole('link', { name: 'Agents' }));
    const row = (await screen.findByRole('link', { name: /Builder/ })).closest('tr')!;
    expect(row).toHaveTextContent('▲');
    expect(row.querySelector('.stuck-mark')).toHaveTextContent('stuck: Planner needs attention');
  });

  it('offers Restart this agent on the agent that needs attention, and calls the API', async () => {
    renderAt('/rig/r/agent/dev.a%40r');
    act(() => FakeEventSource.instances[0].emit('snapshot', stuckSnapshot()));
    const pane = await screen.findByRole('region', { name: 'Agent Planner' });
    const actions = within(pane).getByRole('group', { name: 'Handoffs of Planner' });
    expect(within(actions).queryByRole('button', { name: 'Hand over again' })).toBeNull();
    await userEvent.click(within(actions).getByRole('button', { name: 'Restart this agent' }));
    expect(api.restartSeat).toHaveBeenCalledWith('dev.a@r');
    expect(await within(pane).findByRole('status')).toHaveTextContent('Restarting Planner…');
  });

  it('shows the stuck reason in the pane, and Resend handoff when re-sending can help', async () => {
    renderAt('/rig/r/agent/dev.b%40r');
    const snap = snapshot({
      seats: [seatView('a', { label: 'Planner' }), seatView('b', { label: 'Builder', handoff_stuck: { reason: 'the handoff from Planner was not delivered', can_resend: true } })],
    });
    act(() => FakeEventSource.instances[0].emit('snapshot', snap));
    const pane = await screen.findByRole('region', { name: 'Agent Builder' });
    expect(pane.querySelector('.ap-notices')).toHaveTextContent('▲stuck: the handoff from Planner was not delivered');
    await userEvent.click(within(pane).getByRole('button', { name: 'Resend handoff' }));
    expect(api.resendHandoffs).toHaveBeenCalledWith('dev.b@r');
  });

  it('offers Hand over again on an agent that hands work to others', async () => {
    renderAt('/rig/r/agent/dev.a%40r');
    act(() => FakeEventSource.instances[0].emit('snapshot', waitingSnapshot()));
    const chat = await screen.findByRole('region', { name: 'Agent Planner' });
    // Not the obvious next step here, so it is one step back: on Activity, and in Ctrl K.
    expect(within(chat).queryByRole('button', { name: 'Hand over again' })).toBeNull();
    await userEvent.click(screen.getByRole('link', { name: 'Activity' }));
    const pane = await screen.findByRole('region', { name: 'Activity' });
    const button = within(pane).getByRole('button', { name: 'Hand over again' });
    expect(button).toHaveAttribute('data-tip', "Send Planner's latest finished turn to Builder; every agent downstream runs once");
    // Practice agents are free: no question asked.
    await userEvent.click(button);
    expect(api.handOver).toHaveBeenCalledWith('dev.a@r');
    expect(within(pane).queryByRole('alertdialog')).toBeNull();
    // A stopped team offers neither.
    act(() => FakeEventSource.instances[0].emit('event', { event: { ...ev(30, { type: 'rig-desired-changed', desired: 'stopped' }), subject: { kind: 'rig', id: 'r' } }, seat: null }));
    expect(within(pane).queryByRole('button', { name: 'Hand over again' })).toBeNull();
  });
});

describe('Handoffs: pulse timing', () => {
  it('clears the pulse even when more events arrive while it shows', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', waitingSnapshot()));
    act(() => FakeEventSource.instances[0].emit('event', { event: handedOver(20), seat: seatView('b', { label: 'Builder' }) }));
    const edge = await screen.findByTestId('edge-dev.a@r-dev.b@r');
    expect(edge).toHaveClass('handoff-pulse');
    act(() => {
      vi.advanceTimersByTime(PULSE_MS / 2);
    });
    // Another event (the message being read) re-renders the graph mid-pulse.
    act(() =>
      FakeEventSource.instances[0].emit('event', { event: ev(21, { type: 'message-delivered', message_id: 'h20' }, 'dev.b@r'), seat: seatView('b', { label: 'Builder' }) }),
    );
    expect(edge).toHaveClass('handoff-pulse');
    act(() => {
      vi.advanceTimersByTime(PULSE_MS);
    });
    expect(edge).not.toHaveClass('handoff-pulse');
  });
});

describe('Handoffs: Hand over again on real agents', () => {
  it('asks first how many agents re-run and whose account they use', async () => {
    vi.mocked(api.handOver).mockClear();
    renderAt('/rig/r/agent/dev.a%40r');
    const rig = compiledRig();
    const real = { ...rig, seats: rig.seats.map((s) => ({ ...s, runtime: 'claude-code' as const })) };
    const seats = [seatView('a', { label: 'Planner', runtime: 'claude-code' }), seatView('b', { label: 'Builder', runtime: 'claude-code' })];
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ rig: real, seats })));
    await screen.findByRole('region', { name: 'Agent Planner' });
    // Ctrl K's "Hand over again…" opens Activity and asks the same question as the button.
    const user = userEvent.setup();
    await user.keyboard('{Control>}k{/Control}');
    await user.keyboard('hand over again{Enter}');
    const pane = await screen.findByRole('region', { name: 'Activity' });
    expect(within(pane).getByRole('alertdialog', { name: /^Re-run 1 agent downstream of Planner/ })).toBeInTheDocument();
    await user.click(within(pane).getByRole('button', { name: 'Cancel' }));
    expect(api.handOver).not.toHaveBeenCalled();
    await userEvent.click(within(pane).getByRole('button', { name: 'Hand over again' }));
    const ask = within(pane).getByRole('alertdialog', { name: 'Re-run 1 agent downstream of Planner? Each runs once and uses your Claude account.' });
    expect(within(ask).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await userEvent.click(within(ask).getByRole('button', { name: 'Cancel' }));
    expect(api.handOver).not.toHaveBeenCalled();
    await userEvent.click(within(pane).getByRole('button', { name: 'Hand over again' }));
    await userEvent.click(within(pane).getByRole('button', { name: 'Re-run 1 agent' }));
    expect(api.handOver).toHaveBeenCalledWith('dev.a@r');
  });
});
