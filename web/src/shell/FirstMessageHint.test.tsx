import { act, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { CompiledRig } from '../generated/CompiledRig';
import type { RigSnapshot } from '../generated/RigSnapshot';
import type { SeatView } from '../generated/SeatView';
import { resetSeenCache } from '../seen';
import { fakeTeam, renderAt, resetApi } from '../structure/harness';
import { compiledRig, draftAgent, ev, FakeEventSource, seatView, teamDraft, teamRecord } from '../test-fixtures';
import { ALL_TEAMS_STREAM } from '../useTeams';
import { forgetViewports } from './viewport';
import { firstMessageIsWork } from './model';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'teamSpec'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const HINT = "Your next message is this agent's work and starts the team's handoffs.";

/** Team `t` as the server compiles it: Builder hands work to Reviewer. */
function rigT(): CompiledRig {
  const base = compiledRig();
  const seat = (m: string) => ({ ...base.seats[0], id: `team.${m}@t`, pod: 'team', member: m, label: null, cwd: '.' });
  return {
    ...base,
    name: 't',
    pods: [{ id: 'team', label: 'team' }],
    seats: [seat('builder'), seat('reviewer')],
    edges: [{ kind: 'delegates_to', from: 'team.builder@t', to: 'team.reviewer@t', enforced: true }],
    launch_layers: [['team.builder@t'], ['team.reviewer@t']],
  };
}
const viewT = (m: string, over: Partial<SeatView> = {}): SeatView => ({ ...seatView(m), id: `team.${m}@t`, rig: 't', pod: 'team', ...over });
/** The reviewer waits for the builder: the server's record while the builder has not finished a work turn. */
const waiting = (over: Partial<RigSnapshot> = {}): RigSnapshot => ({
  rig: rigT(),
  desired: 'running',
  seats: [viewT('builder'), viewT('reviewer', { waiting_for: ['team.builder@t'] })],
  last_seq: 5,
  ...over,
});
const rigSummary = { name: 't', desired: 'running' as const, seat_count: 2, running_count: 2, attention_count: 0 };
const stream = () => FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM && !i.closed).at(-1);

/** Team `t` started; the builder's Job is `job`. */
function started(job = '') {
  const draft = teamDraft({ agents: [draftAgent('builder', { job }), draftAgent('reviewer', { x: 300, job: 'Review it' })] });
  fakeTeam(teamRecord({ draft, started_revision: 1, live: true }));
  vi.mocked(api.listRigs).mockResolvedValue([rigSummary]);
  vi.mocked(api.listTeams).mockResolvedValue([{ id: 't', name: 'My team', agent_count: 2, updated_at_ms: 1, started: rigSummary }]);
}

async function openPane(agent: string, snap: RigSnapshot) {
  renderAt(`/rig/t/agent/team.${agent}%40t`);
  await waitFor(() => expect(stream()).toBeDefined());
  act(() => stream()!.emit('snapshot', snap));
  return screen.findByRole('region', { name: `Agent ${agent[0].toUpperCase()}${agent.slice(1)}` });
}

beforeEach(() => {
  resetApi();
  FakeEventSource.reset();
  forgetViewports();
  resetSeenCache();
  localStorage.clear();
});

describe('First message of a first agent with no Job', () => {
  it('says above the message box that the next message is the work, until it has finished that work', async () => {
    started('');
    const pane = await openPane('builder', waiting());
    const hint = await within(pane).findByText(HINT);
    const box = within(pane).getByRole('form', { name: 'Send a message' });
    // Above the message box.
    expect(hint.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Its work turn finished: the reviewer got its handoff and no longer waits.
    act(() =>
      stream()!.emit('event', {
        event: ev(20, { type: 'handoff-waiting', waiting_for: [], stuck: null }, 'team.reviewer@t'),
        seat: viewT('reviewer'),
      }),
    );
    await waitFor(() => expect(within(pane).queryByText(HINT)).toBeNull());
  });

  it('is not shown for an agent with a Job, nor for an agent handed work by another', async () => {
    started('Build it');
    const pane = await openPane('builder', waiting());
    expect(within(pane).getByRole('form', { name: 'Send a message' })).toBeInTheDocument();
    await act(async () => {});
    expect(within(pane).queryByText(HINT)).toBeNull();
  });

  it('is hidden while the agent is in a turn, and once it has read a message in this session', async () => {
    started('');
    const pane = await openPane('builder', waiting({ seats: [viewT('builder', { current_turn: 't1', activity: 'working' }), viewT('reviewer', { waiting_for: ['team.builder@t'] })] }));
    expect(within(pane).getByRole('form', { name: 'Send a message' })).toBeInTheDocument();
    await act(async () => {});
    expect(within(pane).queryByText(HINT)).toBeNull();
    // The turn ends without the work being finished yet (the reviewer still waits), but the
    // message it read was its work: the next one is not.
    act(() =>
      stream()!.emit('event', {
        event: ev(30, { type: 'message-consumed', message_id: 'm1', turn_id: 't1' }, 'team.builder@t'),
        seat: viewT('builder'),
      }),
    );
    await act(async () => {});
    expect(within(pane).queryByText(HINT)).toBeNull();
  });

  it('is hidden when the history of this session shows a message read', async () => {
    started('');
    vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) =>
      id === 'team.builder@t'
        ? [
            ev(1, { type: 'message-consumed', message_id: 'old', turn_id: 't0' }, id),
            ev(2, { type: 'tenure-started', tenure: 2, cause: 'fresh', driver: 'claude', native_session_id: null, caps: seatView('a').caps! }, id),
            ev(3, { type: 'message-consumed', message_id: 'm1', turn_id: 't1' }, id),
          ]
        : [],
    );
    const pane = await openPane('builder', waiting());
    await waitFor(() => expect(api.latestSeatEvents).toHaveBeenCalledWith('team.builder@t'));
    await act(async () => {});
    expect(within(pane).queryByText(HINT)).toBeNull();
  });

  it('is shown when the only message read was in an earlier session', async () => {
    started('');
    vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) =>
      id === 'team.builder@t'
        ? [
            ev(1, { type: 'message-consumed', message_id: 'old', turn_id: 't0' }, id),
            ev(2, { type: 'tenure-started', tenure: 2, cause: 'fresh', driver: 'claude', native_session_id: null, caps: seatView('a').caps! }, id),
          ]
        : [],
    );
    const pane = await openPane('builder', waiting());
    expect(await within(pane).findByText(HINT)).toBeInTheDocument();
  });

  it('is decided by the team design and the waiting record', () => {
    const d = teamDraft();
    const seats = Object.fromEntries(waiting().seats.map((s) => [s.id, s]));
    expect(firstMessageIsWork(rigT(), seats, d, 'team.builder@t')).toBe(true);
    // Handed work by the builder: its message is not its work.
    expect(firstMessageIsWork(rigT(), seats, d, 'team.reviewer@t')).toBe(false);
    // Nobody waits for it any more: it has finished its work turn in this start.
    expect(firstMessageIsWork(rigT(), { ...seats, 'team.reviewer@t': viewT('reviewer') }, d, 'team.builder@t')).toBe(false);
    // A Job is its work.
    const withJob = teamDraft({ agents: [draftAgent('builder', { job: 'Build it' }), draftAgent('reviewer', { x: 300 })] });
    expect(firstMessageIsWork(rigT(), seats, withJob, 'team.builder@t')).toBe(false);
    // No team design: unknown, so no hint.
    expect(firstMessageIsWork(rigT(), seats, undefined, 'team.builder@t')).toBe(false);
  });
});
