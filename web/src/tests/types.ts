// Mirrors arenad's model.rs / fork.rs / compare.rs / recorded.rs (serde snake_case), reached through `/api/arena` (controld `arena.rs`).

export type AgentKind = 'claude_code' | 'codex' | 'open';

/** Model arenad uses for an OpenRouter agent with no model set (`agents/open.rs`). */
export const DEFAULT_OPENROUTER_MODEL = 'deepseek/deepseek-chat-v3.1';
export type BranchStatus = 'running' | 'passed' | 'failed' | 'infra_error' | 'interrupted';
export type EventKind =
  | 'run_start'
  | 'agent_start'
  | 'model_call'
  | 'tool_call'
  | 'handoff'
  | 'agent_end'
  | 'check';

export type RestoreMode = 'exact' | 'approximate';
/** How one part of a rerun's world is (or was) restored (`model.rs` `RestoredHow`). */
export type RestoredHow = 'save' | 'replay' | 'approximate';

export interface RestoredPart {
  part: string;
  how: RestoredHow;
}

/** One part of `GET /api/branches/{id}/restore-plan` (`fork/rerun.rs` `PartPlan`). */
export interface PartPlan extends RestoredPart {
  /** For a replay: a rough estimate of how long it takes. */
  estimate_seconds?: number;
}

export interface RestorePlan {
  parts: PartPlan[];
}

/** `{part: save}`. */
export type Saves = Record<string, string>;

/** A part of a benchmark's world. */
export interface WorldPart {
  part: string;
  integration: string;
  config?: unknown;
}

/** Payload of `run_start` (`runner.rs`); runs recorded before integrations have only `task_id` and `team`. */
export interface RunStartPayload {
  task_id: string;
  team?: Team;
  benchmark?: string;
  world?: WorldPart[];
  seed?: number;
  base_seed?: number;
  /** Each part's integration fingerprint at the run's start. */
  fingerprints?: Record<string, unknown>;
  saves?: Saves;
}

export interface AgentSpec {
  id: string;
  name: string;
  kind: AgentKind;
  model: string | null;
  prompt: string;
}

export interface Team {
  id: string;
  name: string;
  agents: AgentSpec[];
}

export interface Change {
  prompt?: string | null;
  model?: string | null;
  kind?: AgentKind | null;
}

export interface Event {
  branch_id: string;
  seq: number;
  at: string;
  agent: string | null;
  kind: EventKind;
  payload: unknown;
}

export interface ToolCallPayload {
  name: string;
  args: unknown;
  result: unknown;
  error: string | null;
  reads: unknown;
  writes: unknown;
  /** `saves.retail` when that part was saved; runs recorded before integrations have only this. */
  snapshot: string | null;
  snapshot_error?: string;
  tool_call_id: string | null;
  /** The world part the tool ran on. */
  part?: string;
  /** The parts saved after this call (absent when nothing was saved). */
  saves?: Saves;
  /** The world clock when the call ran. */
  clock_ms?: number;
  paused_ms?: number;
}

export interface ModelCallPayload {
  request: unknown;
  response: unknown;
  usage: unknown;
  cost_usd: number | null;
  cost_source: string | null;
}

export interface AgentEndPayload {
  ok: boolean;
  error: string | null;
  tokens: number | null;
  cost_usd: number | null;
  cost_source: string | null;
  seconds: number | null;
}

export interface HandoffPayload {
  note: string;
}

export interface CheckPayload {
  passed: boolean;
  gold_db: string | null;
  differing_rows: string[] | null;
  /** Every part's final state. */
  final_saves?: Saves;
  /** Parts with no expected result to compare with (parts with replay). */
  unchecked?: string[];
}

export interface Branch {
  id: string;
  run_id: string;
  parent_id: string | null;
  start_seq: number | null;
  team: Team;
  changes: Record<string, Change> | null;
  fork_note: string | null;
  status: BranchStatus;
  error: string | null;
  created_at: string;
  ended_at: string | null;
  /** The restore a fork asked for (null for a run's first branch; absent from older arenads). */
  restore_mode?: RestoreMode | null;
  /** How each part of a fork's world was restored (empty for a run's first branch). */
  restored?: RestoredPart[];
}

/** GET /api/branches/{id}: the branch plus its events. */
export interface BranchDetail extends Branch {
  events: Event[];
}

export interface RunSummary {
  id: string;
  task_id: string;
  created_at: string;
  branches: Branch[];
}

export interface ForkResponse {
  branch_id: string;
  start_seq: number;
  moved_from: number | null;
  reason: string | null;
}

export interface Health {
  open_available: boolean;
  claude: boolean;
  codex: boolean;
  disk?: DiskUsage;
  benchmark?: BenchmarkInfo;
}

/** Space used by every save the arena keeps (the hub's and each integration's), and the limit. */
export interface DiskUsage {
  bytes: number;
  limit_bytes: number;
}

/** The benchmark new runs use; `error` when it can't be described. */
export interface BenchmarkInfo {
  name: string;
  version?: string;
  tasks?: number;
  world?: WorldPart[];
  error?: string;
}

export interface TaskSummary {
  id: string;
  reason: string;
}

export interface NewRunResponse {
  run_id: string;
  branch_id: string;
}

/** SSE `status` event payload. */
export interface StatusEvent {
  status: BranchStatus;
  error: string | null;
}

export interface RowsResponse {
  seq: number;
  /** Key `<table>/<id>` to the document at that step, or null if absent. */
  rows: Record<string, unknown>;
}

export interface CompareSide {
  status: BranchStatus;
  restore_mode?: RestoreMode | null;
  restored?: RestoredPart[];
  seconds: number | null;
  tokens: number | null;
  cost_usd: number | null;
  cost_source: string | null;
}

export interface RowDiff {
  key: string;
  a: unknown;
  b: unknown;
}

/** Two branches compared, as the pages show them: the comparison, both branches, their run's branch names and the run. */
export type Comparison = { cmp: CompareResponse; x: BranchDetail; y: BranchDetail; names: Map<string, string>; run: string };

export interface CompareResponse {
  a: CompareSide;
  b: CompareSide;
  first_divergent_seq: number | null;
  differing_rows: RowDiff[];
  /** Parts that keep no saves, so their final states could not be compared. */
  not_compared?: string[];
}

/** How an integration can put a part back (`parts::proto::Restore`). */
export type ExactKind = 'save' | 'replay' | 'none';
export interface PartRestore {
  exact: ExactKind;
  approximate: boolean;
}

/**
 * One part of `GET /api/world` (arenad `api/world.rs` `PartInfo`). A part whose integration did
 * not answer has only `name`, `integration`, `answered: false` and `error`.
 */
export interface WorldPartInfo {
  name: string;
  integration: string;
  answered: boolean;
  error?: string;
  version?: string;
  restore?: PartRestore;
  uses_clock?: boolean;
  tools?: string[];
}

export interface WorldResponse {
  parts: WorldPartInfo[];
}

/** One integration of `GET /api/usage`: the parts that use it and the bytes it keeps, or why it can't say. */
export interface IntegrationUsage {
  integration: string;
  parts: string[];
  bytes: number;
  error?: string;
}

export interface UsageResponse {
  integrations: IntegrationUsage[];
  total_bytes: number;
  limit_bytes: number;
}

/** `POST /api/gc`: the saves whose folders went, the bytes freed, and the integrations whose cleanup failed. */
export interface GcResponse {
  removed: string[];
  freed_bytes: number;
  errors: { integration: string; message: string }[];
}
