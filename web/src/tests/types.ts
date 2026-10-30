// Mirrors arenad's model.rs / fork.rs / compare.rs (serde snake_case), reached through `/api/arena` (controld `arena.rs`).

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
  snapshot: string | null;
  snapshot_error?: string;
  tool_call_id: string | null;
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
}

/** Space used by the arena's branch, snapshot and gold databases. */
export interface DiskUsage {
  databases: number;
  bytes: number;
  limit_bytes: number;
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

export interface CompareResponse {
  a: CompareSide;
  b: CompareSide;
  first_divergent_seq: number | null;
  differing_rows: RowDiff[];
}
