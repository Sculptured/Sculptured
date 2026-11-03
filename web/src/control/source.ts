import type { Step } from '../tests/model';
import type { RestorePlan } from '../tests/types';

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
};

/**
 * One entry of the merged event list, oldest first. `kind` is the record's own type (for filters),
 * `text` plain words, `raw` the record itself (evidence). `alarm` names the alarm rule this entry
 * raises (see alarms.ts), when it raises one.
 */
export type FeedItem = { id: string; at: number; agent?: string; kind: string; text: string; raw: unknown; alarm?: string };

/**
 * A waiting decision that takes approve or reject: a proposed change (`change`) or a request to use
 * a tool (`question`). `title` and `detail` are the agent's own words (agent-reported evidence).
 */
export type Approval = { id: string; agent?: string; title: string; detail?: string; kind: 'change' | 'question' };

/**
 * What an action did, in plain words, with every agent it left alone and why, and (`notes`) anything
 * to know about an agent it did act on.
 */
export type ActionReport = {
  text: string;
  skipped: { agent: string; name: string; reason: string }[];
  notes?: { agent: string; name: string; text: string }[];
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

export type TeamActions = Record<'start' | 'pauseAll' | 'resumeAll' | 'emergencyStop', Action>;

/** A simulated run's timeline: its steps and saves, a cursor, the restore plan and fork. */
export type SimulatedTimeline = {
  steps: Step[];
  saves: number[];
  cursor: number;
  setCursor(seq: number): void;
  plan(seq: number): Promise<RestorePlan>;
  fork(seq: number, changes: unknown, restore: 'exact' | 'approximate'): Promise<{ run: string; branch: string }>;
};

export type ControlSource = {
  kind: 'deployment' | 'simulated';
  title: string;
  /** Some agent has, or may have, a process. */
  running: boolean;
  /** When the earliest current session of a running agent started; absent when unknown. */
  startedAt?: number;
  agents: AgentLine[];
  feed: FeedItem[];
  approvals: Approval[];
  decide(id: string, d: { allow: true } | { allow: false; reason: string }): Promise<void>;
  agentControl(agent: string, action: 'pause' | 'resume' | 'stop'): Action;
  message(agent: string, text: string): Action;
  team: TeamActions;
  /** Tokens and cost are absent when unknown. `toolCallsPerMin`: the last 15 minutes, oldest first. */
  metrics: { tokensIn?: number; tokensOut?: number; costUsd?: number; toolCallsPerMin: number[] };
  /** What could not be loaded, in plain words (the rest of the source is then incomplete). */
  problems?: string[];
  /** Simulated only. */
  timeline?: SimulatedTimeline;
  /** Simulated only: the world's rows at the cursor. */
  world?: { rows(key?: string): Promise<Record<string, unknown>> };
};
