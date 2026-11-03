import { useCallback, useMemo } from 'react';
import { api, type SeatControlMode } from '../api';
import { plural } from '../format';
import type { SeatView } from '../generated/SeatView';
import { useShell, type RunHandle } from '../shell/ShellContext';
import { hasProcess } from '../status';
import { emptyState, type RigState } from '../useRigStream';
import { useTeamEvents } from '../useTeamEvents';
import { displayName, nameForId, type NameMap } from '../vocab';
import { agentLine, agentReport, controlReport, messageCan, NOTHING_RUNNING, pauseCan, resumeCan, stopCan, teamCan, type Modes } from './deploymentAgents';
import { approvalsOf, metricsOf, useDeploymentData } from './deploymentData';
import { feedFrom, startedAt, toolCallsPerMin } from './deploymentFeed';
import type { Action, Availability, ControlSource, TeamActions } from './source';
import { useActionRunner, type MakeAction } from './useActionRunner';

/** Events loaded per agent for the feed (the live stream adds to them). */
const TAIL = 200;
const NO_SEATS: SeatView[] = [];
const NO_NAMES: NameMap = {};

/** Seat ids in the team's order (its snapshot), then any the stream reported since. */
function seatIdsOf(snapshot: RigState['snapshot'], seats: RigState['seats']): string[] {
  const ids = (snapshot?.seats ?? []).map((s) => s.id);
  return [...ids, ...Object.keys(seats).filter((id) => !ids.includes(id))];
}

/**
 * A Sculptured team as a ControlSource: the shell's live stream, recent events, questions, usage,
 * the run budget, and the agent and team control routes.
 */
export function useDeploymentSource(team: string): ControlSource {
  const shell = useShell();
  const { state, connected } = shell.teams[team] ?? { state: emptyState, connected: false };
  const names = shell.names[team] ?? NO_NAMES;
  const ids = useMemo(() => seatIdsOf(state.snapshot, state.seats), [state.snapshot, state.seats]);
  const seats = useMemo(() => (ids.length ? ids.map((id) => state.seats[id]).filter(Boolean) : NO_SEATS), [ids, state.seats]);
  const { events, error } = useTeamEvents(ids, state.feed, state.epoch, TAIL);
  const data = useDeploymentData(team, ids, events);
  const nameOf = useCallback((id: string) => (state.seats[id] ? displayName(state.seats[id], names) : nameForId(id, names)), [state.seats, names]);
  const feed = useMemo(() => feedFrom(events, nameOf, data.questions, data.budget), [events, nameOf, data.questions, data.budget]);
  const rate = useMemo(() => toolCallsPerMin(events, shell.now), [events, shell.now]);
  const act = useActionRunner(data.reload);
  const ctx: Ctx = { team, seats: state.seats, modes: data.modes, nameOf, act };
  const problems = [...(error ? [`Could not load recent events: ${error}`] : []), ...data.problems];
  return {
    kind: 'deployment',
    title: shell.titles[team] ?? team,
    running: seats.some((s) => hasProcess(s.lifecycle) || s.lifecycle === 'attention'),
    startedAt: startedAt(events, seats),
    agents: seats.map((s) => agentLine(s, nameOf(s.id), data.modes[s.id], connected, shell.now)),
    feed,
    approvals: approvalsOf(data.questions),
    decide: (id, d) => decide(act, id, d),
    agentControl: (agent, action) => agentAction(ctx, agent, action),
    message: (agent, text) => messageAction(ctx, agent, text),
    team: teamActions(ctx, seats, !state.snapshot && !connected, shell.refs.run.current),
    metrics: metricsOf(data.usage, data.costEvents, rate),
    problems,
  };
}

type Ctx = { team: string; seats: Record<string, SeatView>; modes: Modes; nameOf: (id: string) => string; act: MakeAction };
const OK: Availability = { ok: true };
const GONE: Availability = { ok: false, why: 'This agent is not part of the team now.' };

async function decide(act: MakeAction, id: string, d: { allow: true } | { allow: false; reason: string }): Promise<void> {
  const answer = d.allow ? { kind: 'allow' as const } : { kind: 'deny' as const, reason: d.reason };
  await act(`decide:${id}`, OK, 'Sending…', async (rid) => void (await api.answer(id, answer, rid))).run();
}

const MODE: Record<'pause' | 'resume' | 'stop', SeatControlMode> = { pause: 'pause', resume: 'resume', stop: 'hard' };
const BUSY = { pause: 'Pausing…', resume: 'Resuming…', stop: 'Stopping…' } as const;

function agentAction(c: Ctx, agent: string, action: 'pause' | 'resume' | 'stop'): Action {
  const seat = c.seats[agent];
  const mode = c.modes[agent];
  const can = !seat ? GONE : action === 'pause' ? pauseCan(seat, mode) : action === 'resume' ? resumeCan(mode) : stopCan(seat);
  return c.act(`${action}:${agent}`, can, BUSY[action], async (rid) => {
    const answer = await api.controlAgent(agent, MODE[action], rid);
    if (action !== 'stop') return agentReport(action === 'pause' ? 'Paused' : 'Resumed', answer, c.nameOf);
  });
}

/** A message is sent as an instruction (intent instruct); it does not interrupt the current turn. */
function messageAction(c: Ctx, agent: string, text: string): Action {
  const seat = c.seats[agent];
  const can = seat ? messageCan(seat, text) : GONE;
  return c.act(`message:${agent}:${text}`, can, 'Sending…', async (rid) => {
    await api.send(agent, { text, intent: 'instruct', interrupt: false }, rid);
  });
}

/**
 * Start goes through the project bar (its save, confirmation and refusals); without one it says
 * where to start. Pause all, Resume all and Emergency stop use the team routes.
 */
function teamActions(c: Ctx, seats: SeatView[], loading: boolean, runHandle: RunHandle | null): TeamActions {
  const can = teamCan(seats, c.modes, loading);
  const startCan: Availability = !runHandle
    ? { ok: false, why: 'Start the team from its project page.' }
    : runHandle.startReason ? { ok: false, why: runHandle.startReason } : OK;
  return {
    start: c.act('team:start', startCan, 'Starting…', async () => runHandle?.askStart()),
    pauseAll: c.act('team:pause', can.pauseAll, 'Pausing…', async (rid) => controlReport('Paused', await api.pauseTeam(c.team, rid), c.nameOf)),
    resumeAll: c.act('team:resume', can.resumeAll, 'Resuming…', async (rid) => controlReport('Resumed', await api.resumeTeam(c.team, rid), c.nameOf)),
    emergencyStop: c.act('team:stop-now', can.emergencyStop, 'Stopping…', async (rid) => {
      const { stopped } = await api.stopTeamNow(c.team, rid);
      return { text: stopped > 0 ? `Stopping ${plural(stopped, 'agent')} now, with every agent they started.` : NOTHING_RUNNING, skipped: [] };
    }),
  };
}
