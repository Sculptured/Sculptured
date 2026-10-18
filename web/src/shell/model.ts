import type { CompiledRig } from '../generated/CompiledRig';
import type { RigSummary } from '../generated/RigSummary';
import type { SeatView } from '../generated/SeatView';
import type { HandoffStuck } from '../generated/HandoffStuck';
import {
  agentState,
  emptyCounts,
  sortAgents,
  toolLabel,
  vendor,
  type AgentActivity,
  type AgentState,
  type Counts,
  type Vendor,
} from '../herdr';
import { isDone } from '../seen';
import type { TeamDraft } from '../generated/TeamDraft';
import { DEFAULT_GROUP_ID, nameMap, seatIdFor } from '../draft/ids';
import type { Teams } from '../useTeams';
import { agentPagePath, agentTabOf, boardPath } from '../paths';
import { displayName, labelNames, nameForId, type NameMap } from '../vocab';

export type AgentRow = {
  id: string;
  rig: string;
  name: string;
  group: string;
  folder: string;
  seat: SeatView | undefined;
  state: AgentState;
  vendor: Vendor;
  /** One short phrase: the current tool, "needs you", "done" or the state word. */
  summary: string;
  lastMs: number | null;
  activity: AgentActivity | undefined;
  /** Where the row leads when it is not a running agent (a team that has not started). */
  href?: string;
  /** Names of the agents this one waits for before its first handoff; empty or absent when not waiting. */
  waitingFor?: string[];
  /** Why this agent's handoffs cannot go on by themselves; absent when they can. */
  stuck?: HandoffStuck;
  /** Names of the agents this one hands work to. */
  handsTo?: string[];
  /** Every agent downstream of this one (a "Hand over again" wave runs each once), and the
   * paid accounts they use ("Claude", "Codex"); empty when all are Practice agents. */
  downstream?: { count: number; accounts: string[] };
  /** The agent's team is set to run (so it can be restarted or hand over again). */
  teamRunning?: boolean;
  /** A first agent with no Job that has not finished its work yet in this start: your next
   * message to it is its work and starts the team's handoffs (`firstMessageIsWork`). */
  firstMessageIsWork?: boolean;
};

/** "stuck: Airframe needs attention". */
export const stuckWords = (s: HandoffStuck) => `stuck: ${s.reason}`;

/** "waiting for Planner, Builder 1". */
export const waitingWords = (names: string[]) => `waiting for ${names.join(', ')}`;

/** What a "hands work to" connection does at run time, for tooltips. */
export const handoffWords = (from: string, to: string) => `${to} starts when ${from} finishes; ${from}'s result is passed along`;

export type TeamRow = {
  rig: string;
  agents: AgentRow[];
  counts: Counts;
  connected: boolean;
  loaded: boolean;
  names: NameMap;
  /** The team's own name (a team built here), else the internal name. */
  title: string;
  /** Where the team header leads. */
  href: string;
  /** A team built here that has never been started: its agents come from the draft. */
  draftOnly?: boolean;
};

export function summaryOf(seat: SeatView | undefined, state: AgentState): string {
  if (state.key === 'working') return seat?.current_tool ? toolLabel(seat.current_tool) : 'working';
  if (state.key === 'slot') return 'waiting for a slot to run';
  return state.word;
}

/**
 * True for a first agent (nobody hands it work) whose Job is empty in the team design, while
 * an agent it hands work to still waits for it. The server records that wait exactly until this
 * agent finishes a work turn in the current start, so then its next message is its work and
 * starts the team's handoffs. Without the team design the Job is unknown: false.
 */
export function firstMessageIsWork(compiled: CompiledRig, seats: Record<string, SeatView>, draft: TeamDraft | undefined, seat: string): boolean {
  const agent = draft?.agents.find((a) => seatIdFor(draft, a.id) === seat);
  if (!agent || agent.job.trim()) return false;
  if (compiled.edges.some((e) => e.kind === 'delegates_to' && e.to === seat && e.from !== seat)) return false;
  return Object.values(seats).some((s) => s.waiting_for?.includes(seat));
}

/** Every seat reachable from `seat` along "hands work to" (not `seat` itself). */
const downstreamCache = new WeakMap<CompiledRig, Map<string, string[]>>();
export function downstreamOf(compiled: CompiledRig, seat: string): string[] {
  let byTeam = downstreamCache.get(compiled);
  if (!byTeam) downstreamCache.set(compiled, (byTeam = new Map()));
  const hit = byTeam.get(seat);
  if (hit) return hit;
  const below = reachable(compiled, seat);
  byTeam.set(seat, below);
  return below;
}

function reachable(compiled: CompiledRig, seat: string): string[] {
  const seen = new Set<string>();
  const todo = [seat];
  while (todo.length) {
    const s = todo.pop()!;
    for (const e of compiled.edges) {
      if (e.kind === 'delegates_to' && e.from === s && e.to !== seat && !seen.has(e.to)) {
        seen.add(e.to);
        todo.push(e.to);
      }
    }
  }
  return [...seen];
}

const runtimes = new WeakMap<CompiledRig, Map<string, string>>();
function runtimeOf(compiled: CompiledRig, seat: string) {
  let m = runtimes.get(compiled);
  if (!m) runtimes.set(compiled, (m = new Map(compiled.seats.map((s) => [s.id, s.runtime]))));
  return m.get(seat);
}

/** One row per agent of a team, in declaration order. */
export function teamAgents(
  rig: string,
  compiled: CompiledRig,
  seats: Record<string, SeatView>,
  activity: Record<string, AgentActivity> = {},
  names: NameMap = {},
): AgentRow[] {
  const groups = Object.fromEntries(compiled.pods.map((p) => [p.id, p.label]));
  return compiled.seats.map((spec): AgentRow => {
    const seat = seats[spec.id];
    const act = activity[spec.id];
    const state = agentState(seat, isDone(spec.id, act?.lastTurnEndSeq ?? null));
    const nameOf = (id: string) => (seats[id] ? displayName(seats[id], names) : nameForId(id, names));
    const waitingFor = (seat?.waiting_for ?? []).map(nameOf);
    const stuck = seat?.handoff_stuck ?? undefined;
    const handsTo = compiled.edges.filter((e) => e.kind === 'delegates_to' && e.from === spec.id && e.to !== spec.id).map((e) => nameOf(e.to));
    const below = downstreamOf(compiled, spec.id);
    return {
      id: spec.id,
      rig,
      name: seat ? displayName(seat, names) : nameForId(spec.id, names),
      group: groups[spec.pod] ?? 'unknown',
      folder: seat?.cwd ?? spec.cwd,
      seat,
      state,
      vendor: vendor(seat?.runtime ?? spec.runtime),
      summary: seat?.stalled && seat.lifecycle === 'running' ? `stalled: ${seat.stalled.reason}` : stuck && state.key === 'stuck' ? stuckWords(stuck) : waitingFor.length > 0 ? waitingWords(waitingFor) : summaryOf(seat, state),
      lastMs: seat?.last_event_ms ?? null,
      activity: act,
      waitingFor,
      stuck,
      handsTo,
      downstream: {
        count: below.length,
        accounts: (['claude-code', 'codex'] as const)
          .filter((r) => below.some((id) => runtimeOf(compiled, id) === r))
          .map((r) => (r === 'codex' ? 'Codex' : 'Claude')),
      },
    };
  });
}

export function buildTeams(
  teams: Teams,
  order: string[],
  activity: Record<string, AgentActivity>,
  extraNames: Record<string, NameMap> = {},
  titles: Record<string, string> = {},
): TeamRow[] {
  const rows: TeamRow[] = [];
  for (const rig of order) {
    const t = teams[rig];
    const snap = t?.state.snapshot;
    if (!snap) {
      rows.push({ rig, agents: [], counts: emptyCounts(), connected: !!t?.connected, loaded: false, names: {}, title: titles[rig] ?? rig, href: boardPath(rig) });
      continue;
    }
    const names = { ...labelNames(snap.rig), ...(extraNames[rig] ?? {}) };
    const running = snap.desired === 'running';
    const agents = teamAgents(rig, snap.rig, t.state.seats, activity, names).map((a) => ({ ...a, teamRunning: running }));
    const counts = emptyCounts();
    for (const a of agents) counts[a.state.key]++;
    // A running team opens on its Run board; any other opens on its design.
    rows.push({ rig, agents: sortAgents(agents, (a) => a.state.key), counts, connected: t.connected, loaded: true, names, title: titles[rig] ?? rig, href: boardPath(rig) });
  }
  return rows;
}

/** The team with the most activity first: needing you, then failed, then working, then recent. */
export function sortTeams(rows: TeamRow[], summaries: RigSummary[] | null): TeamRow[] {
  const score = (r: TeamRow) => {
    const s = summaries?.find((x) => x.name === r.rig);
    const latest = Math.max(0, ...r.agents.map((a) => a.lastMs ?? 0));
    return [
      r.loaded ? r.counts['needs-you'] : (s?.attention_count ?? 0),
      r.counts.failed,
      r.counts.working,
      r.loaded ? r.agents.filter((a) => a.seat?.lifecycle === 'running').length : (s?.running_count ?? 0),
      // Teams that have run before come before designs that never started.
      r.draftOnly ? 0 : 1,
      latest,
    ];
  };
  return rows
    .map((r, i) => ({ r, i, s: score(r) }))
    .sort((a, b) => {
      for (let k = 0; k < a.s.length; k++) if (a.s[k] !== b.s[k]) return b.s[k] - a.s[k];
      return a.r.rig.localeCompare(b.r.rig) || a.i - b.i;
    })
    .map((x) => x.r);
}

/** Not started yet: the agent exists only in the team's design. */
export const NOT_STARTED: AgentState = { key: 'planned', glyph: '○', word: 'not started', tone: 'muted' };


/** A team built here that has not started: its agents, from the draft, all "not started". */
export function draftTeamRow(d: TeamDraft): TeamRow {
  const groups = Object.fromEntries(d.groups.map((g) => [g.id, g.label]));
  const agents: AgentRow[] = d.agents.map((a) => {
    const v = vendor(a.kind === 'claude' ? 'claude-code' : a.kind === 'codex' ? 'codex' : 'stub');
    return {
      id: seatIdFor(d, a.id)!,
      rig: d.id,
      name: a.name.trim() || 'Unnamed agent',
      group: groups[a.group_id ?? DEFAULT_GROUP_ID] ?? '',
      folder: a.folder?.trim() || d.folder?.trim() || '',
      seat: undefined,
      state: NOT_STARTED,
      vendor: v,
      summary: 'not started',
      lastMs: null,
      activity: undefined,
      href: agentPagePath(d.id, seatIdFor(d, a.id)!, 'setup'),
    };
  });
  const counts = emptyCounts();
  counts.planned = agents.length;
  return { rig: d.id, agents, counts, connected: false, loaded: true, names: nameMap(d), title: d.name.trim() || 'Unnamed team', href: boardPath(d.id), draftOnly: true };
}


/** An agent's page; old tab names (`visual`, `events`) open its Activity. */
export function agentPath(rig: string, id: string, tab?: string): string {
  const t = agentTabOf(tab);
  return agentPagePath(rig, id, t.tab) + t.search;
}

