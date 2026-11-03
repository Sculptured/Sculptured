import { useCallback, useMemo, type RefObject } from 'react';
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
import { ActionError, type Action, type Approval, type Availability, type ControlSource, type TeamActions } from './source';
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

/** The team's live state from the shell: its agents in order, their names and the team's title. */
function useTeamView(team: string) {
  const shell = useShell();
  const { state, connected } = shell.teams[team] ?? { state: emptyState, connected: false };
  const names = shell.names[team] ?? NO_NAMES;
  const ids = useMemo(() => seatIdsOf(state.snapshot, state.seats), [state.snapshot, state.seats]);
  const seats = useMemo(() => (ids.length ? ids.map((id) => state.seats[id]).filter(Boolean) : NO_SEATS), [ids, state.seats]);
  const nameOf = useCallback((id: string) => (state.seats[id] ? displayName(state.seats[id], names) : nameForId(id, names)), [state.seats, names]);
  return { shell, state, connected, ids, seats, nameOf, title: shell.titles[team] ?? team };
}

/**
 * A Sculptured team as a ControlSource: the shell's live stream, recent events, questions, usage,
 * the run budget, and the agent and team control routes.
 */
export function useDeploymentSource(team: string): ControlSource {
  const { shell, state, connected, ids, seats, nameOf, title } = useTeamView(team);
  const { events, error } = useTeamEvents(ids, state.feed, state.epoch, TAIL);
  const data = useDeploymentData(team, ids, events);
  const feed = useMemo(
    () => feedFrom(events, { nameOf, title, seats: state.seats, questions: data.questions, budget: data.budget }),
    [events, nameOf, title, state.seats, data.questions, data.budget],
  );
  const rate = useMemo(() => toolCallsPerMin(events, shell.now), [events, shell.now]);
  const act = useActionRunner(data.reload);
  const ctx: Ctx = { team, seats: state.seats, modes: data.modes, nameOf, act };
  const approvals = approvalsOf(data.questions);
  const problems = [...(error ? [`Could not load recent events: ${error}`] : []), ...data.problems];
  return {
    kind: 'deployment',
    title,
    running: seats.some((s) => hasProcess(s.lifecycle) || s.lifecycle === 'attention'),
    startedAt: startedAt(events, seats),
    agents: seats.map((s) => agentLine(s, nameOf(s.id), data.modes[s.id], connected, shell.now)),
    feed,
    approvals,
    decide: (id, d) => decide(act, approvals, id, d),
    agentControl: (agent, action) => agentAction(ctx, agent, action),
    message: (agent, text) => messageAction(ctx, agent, text),
    team: teamActions(ctx, seats, !state.snapshot && !connected, shell.refs.run),
    metrics: metricsOf(data.usage, data.costEvents, rate),
    problems,
  };
}

type Ctx = { team: string; seats: Record<string, SeatView>; modes: Modes; nameOf: (id: string) => string; act: MakeAction };
const OK: Availability = { ok: true };
const no = (why: string): Availability => ({ ok: false, why });
const GONE: Availability = { ok: false, why: 'This agent is not part of the team now.' };

export const ANSWER_IN_NEEDS_YOU = 'Answer this in Needs you';

/**
 * Approve or reject. A question that needs a written answer is refused (it is answered in Needs you).
 * The decision is part of the action's key, so a different decision is a new request.
 */
async function decide(act: MakeAction, approvals: Approval[], id: string, d: { allow: true } | { allow: false; reason: string }): Promise<void> {
  if (approvals.find((a) => a.id === id)?.kind === 'answer') throw new ActionError(ANSWER_IN_NEEDS_YOU);
  const answer = d.allow ? { kind: 'allow' as const } : { kind: 'deny' as const, reason: d.reason };
  const key = `decide:${id}:${d.allow ? 'allow' : `deny:${d.reason}`}`;
  await act(key, OK, 'Sending…', async (rid) => void (await api.answer(id, answer, rid))).run();
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

export const START_ELSEWHERE = 'Start the team from its project page.';
export const STOP_ELSEWHERE = 'Stop the team from its project page.';

/**
 * Start and Stop team… go through the project bar (its save, confirmations and refusals): the bar's
 * reason disables them up front and is read again when pressed; without a bar they say where to go.
 * Stop team… is the bar's graceful stop (agents finish, then stop), offered while anything may run.
 */
function barActions(c: Ctx, can: ReturnType<typeof teamCan>, run: RefObject<RunHandle | null>): Pick<TeamActions, 'start' | 'stop'> {
  const bar = run.current;
  const startCan: Availability = can.start.ok && bar?.startReason ? no(bar.startReason) : can.start;
  // Stop is offered whenever Emergency stop is (something may have a process), unless the bar says why not.
  const stopCan: Availability = !can.emergencyStop.ok ? can.emergencyStop : bar?.stopReason ? no(bar.stopReason) : OK;
  return {
    start: c.act('team:start', startCan, 'Starting…', async () => {
      const now = run.current;
      if (!now) return { text: START_ELSEWHERE, skipped: [], refused: true };
      if (now.startReason) return { text: now.startReason, skipped: [], refused: true };
      now.askStart();
    }),
    stop: c.act('team:stop', stopCan, 'Asking…', async () => {
      const now = run.current;
      if (!now) return { text: STOP_ELSEWHERE, skipped: [], refused: true };
      if (now.stopReason) return { text: now.stopReason, skipped: [], refused: true };
      now.askStop();
    }),
  };
}

/** Pause all, Resume all and Emergency stop use the team routes. */
function teamActions(c: Ctx, seats: SeatView[], loading: boolean, run: RefObject<RunHandle | null>): TeamActions {
  const can = teamCan(seats, c.modes, loading);
  return {
    ...barActions(c, can, run),
    pauseAll: c.act('team:pause', can.pauseAll, 'Pausing…', async (rid) => controlReport('Paused', await api.pauseTeam(c.team, rid), c.nameOf)),
    resumeAll: c.act('team:resume', can.resumeAll, 'Resuming…', async (rid) => controlReport('Resumed', await api.resumeTeam(c.team, rid), c.nameOf)),
    emergencyStop: c.act('team:stop-now', can.emergencyStop, 'Stopping…', async (rid) => {
      const { stopped } = await api.stopTeamNow(c.team, rid);
      return { text: stopped > 0 ? `Stopping ${plural(stopped, 'agent')} now, with every agent they started.` : NOTHING_RUNNING, skipped: [] };
    }),
  };
}
