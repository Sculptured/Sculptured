import type { CompiledEdge } from './generated/CompiledEdge';
import type { CompiledRig } from './generated/CompiledRig';
import type { ConnectionKind } from './generated/ConnectionKind';
import type { Desired } from './generated/Desired';
import type { EdgeKind } from './generated/EdgeKind';
import type { Runtime } from './generated/Runtime';
import type { SeatView } from './generated/SeatView';

/**
 * Every user-facing word for teams and agents lives here. OpenRig terms (rig, pod, seat,
 * member, edge, runtime, RigSpec, YAML) may appear only inside an Advanced disclosure.
 */

export const RUNTIME_LABEL: Record<Runtime, string> = {
  'claude-code': 'Claude',
  codex: 'Codex',
  pi: 'Pi',
  omp: 'OMP',
  terminal: 'Terminal',
  stub: 'Practice agent',
};

export const KIND_LABEL: Record<'claude' | 'codex' | 'practice', string> = {
  claude: 'Claude',
  codex: 'Codex',
  practice: 'Practice agent',
};

export const EDGE_WORDS: Record<EdgeKind, string> = {
  delegates_to: 'hands work to',
  escalates_to: 'reports to',
  can_observe: 'watches',
  spawned_by: 'was started by',
  collaborates_with: 'works with',
};

export const NOT_ENFORCED = 'not enforced';

/** Why an agent's session began, in plain words (the server's `TenureCause`). */
export const CAUSE_WORDS: Record<string, string> = {
  fresh: 'new conversation',
  resume: 'resumed its last conversation',
  handover: 'handed over',
  fork: 'forked',
};

/** The program that runs an agent's session, by the name the runner reports. */
export const DRIVER_WORDS: Record<string, string> = {
  stub: 'Practice agent',
  claude: 'Claude',
  'claude-code': 'Claude',
  codex: 'Codex',
};

/** The one user-facing noun for what the sidebar lists: a project holds a team's design and history. */
export const PROJECT_WORDS = {
  noProjects: 'No projects yet.',
  allProjects: 'all projects',
  allProjectsTip: 'Every project on this computer',
  newProject: 'New project',
  couldNotLoad: 'Could not load projects',
} as const;

/** Agent display names keyed by internal address (seat id). Teams pass names from their draft. */
export type NameMap = Record<string, string>;

/** The one way an agent is named: the team's name for it, else its label, else its short id. */
export function displayName(seat: Pick<SeatView, 'id' | 'label' | 'member'>, names: NameMap = {}): string {
  return names[seat.id] ?? (seat.label?.trim() || seat.member);
}

/** A name for an internal address when no seat view is at hand (connections, placeholders). */
export function nameForId(id: string, names: NameMap = {}): string {
  if (names[id]) return names[id];
  const logical = id.split('@')[0];
  return logical.split('.').pop() ?? logical;
}

/** Labels carried by a compiled team, as a NameMap. */
export function labelNames(rig: CompiledRig): NameMap {
  const out: NameMap = {};
  for (const s of rig.seats) if (s.label?.trim()) out[s.id] = s.label.trim();
  return out;
}

/** "Builder hands work to Reviewer", with "(not enforced)" for design-intent connections. */
export function edgeSentence(e: CompiledEdge, names: NameMap = {}): string {
  const text = `${nameForId(e.from, names)} ${EDGE_WORDS[e.kind]} ${nameForId(e.to, names)}`;
  return e.enforced ? text : `${text} (${NOT_ENFORCED})`;
}

export const WORDS = {
  allTeams: 'All teams',
  advanced: 'Advanced',
  internalId: 'Internal id',
  stopTeam: 'Stop team…',
  confirmStopTeam: 'Confirm stop team',
  stopTeamConsequence: (team: string) => `Stops every running agent in ${team}. Their history is kept.`,
  stoppingTeam: (team: string) => `Stopping ${team}…`,
  teamCounts: (running: number, total: number) => `${running} of ${total} agents running`,
  needAttention: (n: number) => `${n} ${n === 1 ? 'needs' : 'need'} attention`,
  desired: (d: Desired) => (d === 'running' ? 'set to run' : 'set to stop'),
  connecting: (team: string) => `Connecting to ${team}…`,
  groupRegion: (label: string) => `group ${label}`,
  connections: 'Connections',
  noConnections: 'No connections.',
  unknownAgent: (name: string) => `${name} — unknown`,
  notStartedAgent: (name: string) => `${name} — not started yet`,
  notInTeam: (name: string, team: string) => `${name} is not part of ${team}.`,
  statusUnknown: (name: string) => `${name} is part of this team but its status is unknown.`,
  inspectorFor: (name: string) => `Inspector for ${name}`,
  closeInspector: 'Back to the team',
  session: (n: number) => (n > 0 ? `session #${n}` : 'not started yet'),
  sessionStarted: (n: number, cause: string, driver: string) => `Session #${n} started · ${CAUSE_WORDS[cause] ?? cause} · ${DRIVER_WORDS[driver] ?? driver}`,
  sessionEnded: (n: number, exit: number | null, signal: number | null) =>
    `Session #${n} ended · exit ${exit ?? 'unknown'}${signal !== null ? ` · signal ${signal}` : ''}`,
  /** A run of session events folded to one line; the verbatim chain opens on click. */
  sessionChain: (started: boolean, ended: boolean, cause: string | undefined, last: string) =>
    started && ended ? `Restarted · ${CAUSE_WORDS[cause ?? ''] ?? cause ?? 'new session'}`
      : started ? `Started · ${CAUSE_WORDS[cause ?? ''] ?? cause ?? 'new session'}`
      : ended ? 'Session ended'
      : last,
  showSessionEvents: 'Show each event',
  agentControls: 'Agent controls',
  stopAgent: 'Hard stop…',
  confirmStopAgent: 'Confirm stop agent',
  stopAgentConsequence: (name: string, inTurn: boolean) =>
    `Stops ${name}. ${inTurn ? '1 turn in progress will be cut off.' : 'No turn in progress.'} Messages it has not read yet will be marked as not delivered.`,
  agentAlreadyStopping: 'This agent is already stopping.',
  noProcessToStop: (status: string) => `Status: ${status}; there is no process to stop.`,
  agentsNotStopped: (n: number) => `${n === 1 ? '1 agent' : `${n} agents`} could not be stopped:`,
  agentsNotStoppedLabel: 'Agents not stopped',
  setupNotes: 'Advanced: setup notes',
  sendNeedsRunning: (status: string) => `Status: ${status}; messages can only be sent to a running agent.`,
  sentAtStart: 'sent at start',
  firstMessageIsWork: "Your next message is this agent's work and starts the team's handoffs.",
  cannotInterrupt: "This kind of agent can't be interrupted; wait for its turn to end, or stop it.",
  interruptUnknown: 'Whether this agent can be interrupted is unknown until it reports what it can do.',
} as const;

export const CONNECTION_WORDS: Record<ConnectionKind, string> = {
  'hands-work-to': 'hands work to',
  'reports-to': 'reports to',
  watches: 'watches',
};

/** Community pages' words. */
export const COMMUNITY_WORDS = {
  tabs: { explore: 'Explore', people: 'People', saved: 'Saved', yours: 'Yours', moderation: 'Moderation' },
  laterVersion: 'Opens in a later version.',
  addToProject: 'Add to project',
  saveToLibrary: 'Save to Library',
  inLibrary: 'In your Library',
  updateTo: (n: number) => `Update to v${n}`,
  save: 'Save',
  unsave: 'Remove save',
  signInToSave: 'Sign in to save',
  supplied: 'Community supplied instructions. Publishing is not verification. Review them before starting agents.',
  optIn: {
    title: 'Turn on Community',
    intro: 'Community is where people share blocks of agents and their profiles. It runs on Sculptured’s servers, so this computer sends nothing there until you turn it on.',
    sendsTitle: 'What turning it on sends',
    sends: ['The Community pages and searches you open.', 'If you sign in: what you publish, save and report, and what you put on your profile.'],
    neverTitle: 'What never leaves this computer',
    never: 'Your projects, files, transcripts and runs.',
    browse: 'Turn on and browse',
    signIn: 'Turn on and sign in',
    otherServer: 'Use another Sculptured server',
  },
  signIn: {
    title: 'Sign in to Sculptured',
    enter: 'Enter this code on the sign-in page:',
    open: 'Open the sign-in page',
    waiting: 'Waiting for you to approve…',
    expired: 'The code expired before it was approved.',
    denied: 'The sign-in was turned down.',
    again: 'Get a new code',
    done: (name: string) => `Signed in as ${name}`,
  },
  turnOff: 'Turn off Community',
};
