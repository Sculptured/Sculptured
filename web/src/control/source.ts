import type { Layout, Step } from '../tests/model';
import type { Branch, BranchDetail, Change, Comparison, RestoreMode, RestorePlan, WorldPartInfo } from '../tests/types';

/**
 * What the Control panes read: one interface over a deployment (a Sculptured team running for real)
 * and a simulated deployment (an Arena run branch). Panes never call the server themselves.
 */

/**
 * Whether an action can run now. When it cannot, `why` says so in plain words shown to the user.
 * `note` (optional) says what it will do, for a confirm step ("Stops 3 agents").
 */
export type Availability = { ok: true; note?: string } | { ok: false; why: string };

export type Tone = 'ok' | 'warn' | 'bad' | 'idle' | 'unknown';

/** One agent: its status as a glyph plus a word (never colour alone), and what it is doing. */
export type AgentLine = {
  id: string;
  name: string;
  status: { glyph: string; word: string; tone: Tone };
  /** What it is doing, in words (protocol-observed); absent when unknown. */
  activity?: string;
  /** Running totals as its CLI reported them; absent when unknown (never 0 for unknown). */
  tokens?: { in: number; out: number };
  /**
   * The alarm rule its status raises now (failed, attention), with a key that names the condition
   * stably (the same key its event uses), so the alarm keeps its id whether it comes from here or
   * from the feed, and while the stream is disconnected.
   */
  alarm?: { rule: string; key: string };
};

/**
 * One entry of the merged event list, oldest first. `kind` is the record's own type (for filters),
 * `text` plain words, `raw` the record itself (evidence). `alarm` names the alarm rule this entry
 * raises (see alarms.ts), when it raises one; `alarmKey` names the condition it reports (a question,
 * an agent's session) so its alarm id stays the same however the condition is seen. Without one the
 * entry's id is the key.
 */
export type FeedItem = { id: string; at: number; agent?: string; kind: string; text: string; raw: unknown; alarm?: string; alarmKey?: string };

/**
 * Something an agent waits on the operator for: a proposed change (`change`) or a request to use a
 * tool (`question`), both decided by approve or reject, or a question that needs a written answer
 * (`answer`), which `decide` refuses: it is answered in Needs you. `title` and `detail` are the agent's
 * own words (agent-reported evidence).
 */
export type Approval = {
  id: string;
  agent?: string;
  title: string;
  detail?: string;
  kind: 'change' | 'question' | 'answer';
  /** Whether the operator can decide it here; when not, the pane shows `why` instead of the buttons. */
  can?: Availability;
};

/**
 * What an action did, in plain words, with every agent it left alone and why, and (`notes`) anything
 * to know about an agent it did act on. `refused`: it did nothing, and `text` says why (it could not
 * run); never shown as a success. A skipped agent's `raw` reason is the server's own words (evidence),
 * as are `notes`.
 */
export type ActionReport = {
  text: string;
  skipped: { agent: string; name: string; reason: string; raw?: true }[];
  notes?: { agent: string; name: string; text: string }[];
  refused?: true;
};

/**
 * An action and whether it can run. `run()` never calls the server when `can` is not ok: it resolves
 * with a report that says why. Pressed again while running, it returns the same promise. It rejects
 * only with an `ActionError`, whose message is already in plain words.
 */
export type Action = { can: Availability; run(): Promise<ActionReport | void> };

/** A failed action, in plain words (server errors went through errors.ts). */
export class ActionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ActionError';
  }
}

/** `stop` is the graceful stop (agents finish their turn, then stop); `emergencyStop` stops them at once. */
export type TeamActions = Record<'start' | 'stop' | 'pauseAll' | 'resumeAll' | 'emergencyStop', Action>;

/**
 * A simulated run's timeline: its steps (in order) and saves (the steps after which some part was
 * saved), a cursor (a step's seq), the restore plan and fork. `layout` is the same steps in lanes,
 * one per agent then the check. `plan` asks for an exact restore unless told otherwise. A fork's
 * `notice` says where it started, in plain words.
 */
export type SimulatedTimeline = {
  steps: Step[];
  saves: number[];
  cursor: number;
  setCursor(seq: number): void;
  plan(seq: number, restore?: RestoreMode): Promise<RestorePlan>;
  fork(seq: number, changes: Record<string, Change>, restore: RestoreMode): Promise<{ run: string; branch: string; notice: string }>;
  layout: Layout;
};

/**
 * The Arena run behind a simulated source: the branch (absent until it loads, or when it can't),
 * its run's branches with their short names, what each part of the world says about itself (null
 * until Arena answers), and whether OpenRouter agents can run. `error` is why the branch is not
 * on screen (Arena not answering, or another failure); `compare(other)` reads another branch against
 * this one (so its change column is what this branch changed).
 */
export type SimulatedRun = {
  run: string;
  branchId: string;
  branch?: BranchDetail;
  branches: Branch[];
  names: Map<string, string>;
  parts: WorldPartInfo[] | null;
  openAvailable: boolean;
  error?: unknown;
  /** The task's own words for what the run had to do, when Arena has them. */
  reason?: string;
  /** Counts Arena answering again after not answering: panes read again when it changes. */
  reconnects: number;
  compare(other: string): Promise<Comparison>;
};

export type ControlSource = {
  kind: 'deployment' | 'simulated';
  title: string;
  /** Some agent has, or may have, a process. */
  running: boolean;
  /** When the earliest current session of a running agent started; absent when unknown. */
  startedAt?: number;
  /** When a run that has finished ended (a simulated branch); the header then shows how long it ran. */
  endedAt?: number;
  /** Nothing about the run could be read yet (Arena not answering, or still loading): readings say unknown, not none. */
  unread?: true;
  /** The run's own status when the source knows it better than its agents say (a simulated branch's outcome). */
  runStatus?: { glyph: string; word: string; tone: Tone };
  agents: AgentLine[];
  feed: FeedItem[];
  approvals: Approval[];
  decide(id: string, d: { allow: true } | { allow: false; reason: string }): Promise<void>;
  agentControl(agent: string, action: 'pause' | 'resume' | 'stop'): Action;
  message(agent: string, text: string): Action;
  team: TeamActions;
  /**
   * Tokens and cost are absent when unknown. `tokens` is the total where only that was reported (an
   * Arena run's agents). `toolCallsPerMin`: the last 15 minutes, oldest first.
   */
  metrics: { tokensIn?: number; tokensOut?: number; tokens?: number; costUsd?: number; toolCallsPerMin: number[] };
  /** What could not be loaded, in plain words (the rest of the source is then incomplete). */
  problems?: string[];
  /** Simulated only. */
  timeline?: SimulatedTimeline;
  /**
   * Simulated only: the world's rows at the cursor's save, for `key`, or else for every key the run
   * had touched by then. `save` is that save's step (null before anything was saved). `read` reads
   * rows `keys` as saved just after any step `seq`, in `part` (else the step's part).
   */
  world?: {
    save: number | null;
    rows(key?: string): Promise<Record<string, unknown>>;
    read(seq: number, keys: string[], part?: string): Promise<Record<string, unknown>>;
  };
  /** Simulated only: the Arena run. */
  arena?: SimulatedRun;
};
