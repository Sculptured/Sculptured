import { track } from './hosted/analytics';
import type { RigSummary } from './generated/RigSummary';
import type { EventRecord } from './generated/EventRecord';
import type { Diagnostic } from './generated/Diagnostic';
import type { MessageIntent } from './generated/MessageIntent';
import type { StopRigOutcome } from './generated/StopRigOutcome';
import type { DraftIssue } from './generated/DraftIssue';
import type { FolderEntry } from './generated/FolderEntry';
import type { FolderListing } from './generated/FolderListing';
import type { TeamDraft } from './generated/TeamDraft';
import type { WorkflowRun } from './generated/WorkflowRun';
import type { Workflow } from './generated/Workflow';
import type { TeamImport } from './generated/TeamImport';
import type { TeamRecord } from './generated/TeamRecord';
import type { TeamSpec } from './generated/TeamSpec';
import type { TeamSummary } from './generated/TeamSummary';
import type { TemplateInfo } from './generated/TemplateInfo';
import type { Answer } from './generated/Answer';
import type { InboxItem } from './generated/InboxItem';
import type { QuestionView } from './generated/QuestionView';
import type { Settings } from './generated/Settings';
import type { TeamSettings } from './generated/TeamSettings';
import type { RestoreDecision } from './generated/RestoreDecision';

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
    public diagnostics?: Diagnostic[],
    /** Plain-language problems pinned to draft elements (a team that cannot start). */
    public issues?: DraftIssue[],
    /** What a tool answered when it could not be used (evidence for the user). */
    public raw?: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/** Rows per history page; a full page means older events exist that were not loaded. */
export const HISTORY_LIMIT = 500;

/** Whose Claude Code and Codex: a computer (by id), a team's computer, or the only connected one. */
export type CliOn = string | { team: string } | undefined;
const cliQuery = (on: CliOn) => (typeof on === 'string' ? `?host=${encodeURIComponent(on)}` : on ? `?team=${encodeURIComponent(on.team)}` : '');

/** One id per user action; reusing it makes a retry or a double-click execute once. */
export function newRequestId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  return `req-${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
}

/** `keepalive` lets a request outlive the page (a save sent as the page goes away). */
/** `text`: send this body as-is with its content type (a file), not as JSON. `bytes`: the same for a picture. */
type RequestOptions = { keepalive?: boolean; text?: { body: string; type: string }; bytes?: { body: Blob; type: string }; signal?: AbortSignal };

let csrfToken: string | null = null;
/** Hosted beta: the gateway's per-session CSRF token, sent with every write. */
export function setCsrf(token: string | null) { csrfToken = token; }

export async function apiRequest<T>(method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE', url: string, body?: unknown, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = {};
  const raw = opts.text ?? opts.bytes;
  if (raw) headers['content-type'] = raw.type;
  else if (body !== undefined) headers['content-type'] = 'application/json';
  if (method !== 'GET' && csrfToken) headers['x-csrf-token'] = csrfToken;
  const res = await fetch(url, {
    method,
    ...(opts.keepalive ? { keepalive: true } : {}),
    ...(opts.signal ? { signal: opts.signal } : {}),
    headers: Object.keys(headers).length ? headers : undefined,
    body: raw ? raw.body : body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json;
  try {
    json = text ? JSON.parse(text) : {};
  } catch {
    // Not JSON: an axum extractor rejection (text/plain) or an HTML page.
    throw new ApiError(res.status, 'http_error', text.slice(0, 200));
  }
  if (json === null || typeof json !== 'object') {
    // Valid JSON but not an object or array (null, a number, a string).
    throw new ApiError(res.status, 'http_error', text.slice(0, 200));
  }
  if (!res.ok) {
    // An answer in another shape (an older Community's `{"error":"text"}`) is never shown as is.
    if (typeof json.error === 'string') throw new ApiError(res.status, 'community_bad_answer', '');
    const e = json.error ?? {};
    throw new ApiError(res.status, e.code ?? 'http_error', e.message ?? `HTTP ${res.status}`, e.diagnostics, e.issues, e.raw);
  }
  return json as T;
}

const request = apiRequest;
const get = <T>(url: string) => request<T>('GET', url);

/** A POST that carries the action's request id first, then its fields. */
const post = <T>(url: string, requestId: string, fields: object = {}) =>
  request<T>('POST', url, { request_id: requestId, ...fields });

const rigUrl = (name: string) => `/api/rigs/${encodeURIComponent(name)}`;
const seatUrl = (id: string) => `/api/seats/${encodeURIComponent(id)}`;

const teamUrl = (id: string) => `/api/teams/${encodeURIComponent(id)}`;
const questionUrl = (id: string) => `/api/questions/${encodeURIComponent(id)}`;

/** What the generator is told before it drafts a team (all optional but the CLI). */
export type GenerateOptions = {
  cli: 'claude' | 'codex';
  model: string | null;
  size: '2-3' | '4-6' | '7-10' | null;
  kinds: 'claude' | 'codex' | 'mixed' | 'practice' | null;
  shape: 'chain' | 'lead' | 'reviewers' | null;
  checks: boolean;
  folder: string | null;
  host?: string | null;
  team?: string | null;
};
/** One drafted team: a `sculptured.team/v1` file, what was left out, and its issues. */
export type GeneratedTeam = { label: string; rationale: string; file: TeamFileV1; dropped: string[]; issues: DraftIssue[] };
export type TeamFileV1 = { format: string; team: import('./generated/TeamDraft').TeamDraft; settings: { posture?: string | null; own_copies: boolean; question_timeout_min?: number | null } };

/** A team's trace export, as the server shows it (never the key). */
export type TraceExportView = { enabled: boolean; endpoint: string; key_saved: boolean; content: 'metadata' | 'full'; last_error: string | null; last_sent_ms: number | null };
/** Saving it: no `key` keeps the saved one, '' removes it. */
export type SaveTraces = { enabled: boolean; endpoint: string; content: 'metadata' | 'full'; key?: string };
/** One turn of a design-assistant conversation. */
export type AssistantTurn = { role: 'person' | 'assistant'; text: string };
/** The assistant's proposed design: nothing is saved until the person approves it. */
export type DesignProposal = { summary: string; madl: string; draft: TeamDraft; dropped: Diagnostic[]; issues: DraftIssue[] };

export type StartResult = { rig: string; warnings: DraftIssue[] };

export type SendBody = { text: string; intent: MessageIntent; interrupt: boolean };
type Empty = Record<string, never>;

export const api = {
  /** Hosted: `host` picks the computer (else the only connected one). */
  clis: (on?: CliOn) => get<import('./generated/CliStatus').CliStatus[]>(`/api/clis${cliQuery(on)}`),
  /** Connected computers (agent runners) with their drivers and, for connected computers, what they allow. */
  hosts: () => get<{ id: string; drivers: string[]; policy: import('./generated/HostPolicyInfo').HostPolicyInfo | null; max_working?: number | null }[]>('/api/hosts'),
  /** Most agents working at once on a computer, across its teams (null = no cap). */
  setHostLimit: (id: string, max_working: number | null) => request<{ id: string; max_working: number | null }>('PUT', `/api/hosts/${encodeURIComponent(id)}/limit`, { max_working }),
  planUsage: () => get<import('./generated/PlanUsageView').PlanUsageView[]>('/api/plan-usage'),
  teamUsage: (id: string) => get<import('./generated/TeamUsage').TeamUsage>(`${teamUrl(id)}/usage`),
  /** Where a run's tokens went (its latest run, or the one starting at `start`). */
  teamReport: (id: string, start?: number) => get<import('./generated/RunReport').RunReport>(`${teamUrl(id)}/report${start ? `?start=${start}` : ''}`),
  /** Your own use across every team: tokens, costs, by day, by CLI and by team. */
  accountUsage: () => get<import('./generated/AccountUsage').AccountUsage>('/api/account/usage'),
  cliLogin: (name: string, on?: CliOn) => post<import('./generated/LoginStarted').LoginStarted>(`/api/clis/${name}/login${cliQuery(on)}`, newRequestId()),
  cliLoginCode: (name: string, code: string, on?: CliOn) => post(`/api/clis/${name}/login/code${cliQuery(on)}`, newRequestId(), { code }),
  deployments: (id:string,before?:number) => get<import('./generated/DeploymentRun').DeploymentRun[]>(`${teamUrl(id)}/deployments${before ? `?before=${before}` : ''}`),
  deployment: (id:string,seq:number) => get<import('./generated/DeploymentHistory').DeploymentHistory>(`${teamUrl(id)}/deployments/${seq}`),
  deploymentEvents: (id:string,seq:number,seat:string,before?:number) => get<import('./generated/DeploymentEvents').DeploymentEvents>(`${teamUrl(id)}/deployments/${seq}/events?seat=${encodeURIComponent(seat)}${before ? `&before=${before}` : ''}`),
  teamBudget: (id: string) => get<import("./generated/TeamBudget").TeamBudget>(`${teamUrl(id)}/budget`),
  /** A run paused at its plan budget: `continue` now, `stop`, or set how it continues (`mode`). */
  budgetPaused: (id: string, action: 'continue' | 'stop' | 'mode', continue_with?: 'notes' | 'resume') => post(`${teamUrl(id)}/budget/paused`, newRequestId(), { action, continue_with }),
  setTeamBudget: (id: string, output_limit: number | null) => post(`${teamUrl(id)}/budget`, newRequestId(), {output_limit}),
  /** Raises a run limit for this run only; a run stopped or wrapping up at it continues. */
  raiseLimit: (id: string, r: { limit: string; hard: number; provider: string | null; window: string | null }) => post(`${teamUrl(id)}/budget/raise`, newRequestId(), r),
  /** Raises one agent's own limit (tokens, turns or minutes) for this run only. */
  raiseAgentLimit: (id: string, member: string, r: { limit: 'tokens' | 'turns' | 'minutes'; hard: number }) => post(`${teamUrl(id)}/agents/${encodeURIComponent(member)}/limits/raise`, newRequestId(), r),
  managedChildren: (id:string) => get<import('./generated/ManagedChild').ManagedChild[]>(`${teamUrl(id)}/children`),
  /** The team's latest message, handoff and start events; `usage` adds its model-usage reports. */
  communications: (id: string, usage = false) => get<EventRecord[]>(`${teamUrl(id)}/communications${usage ? '?usage=true' : ''}`),
  /** The latest run's message, handoff and start events with long text cut: what the Run board's arrows count. */
  runCommunications: (id: string) => get<EventRecord[]>(`${teamUrl(id)}/communications?run=latest&brief=true`),
  /** The same, with the latest run's model-usage reports (tokens, and cost where a source prices it). */
  runUsage: (id: string) => get<EventRecord[]>(`${teamUrl(id)}/communications?run=latest&brief=true&usage=true`),
  agentControl: (id: string) => get<{mode: string | null}>(`${seatUrl(id)}/control`),
  controlAgent: (id: string, mode: "soft" | "hard" | "resume", requestId: string) => post(`${seatUrl(id)}/control`, requestId, {mode}),
  workflow: (id: string) => get<{ config: Workflow | null; run: WorkflowRun | null; active: boolean; usage?: Record<string, import('./generated/TurnTokens').TurnTokens> }>(`${teamUrl(id)}/workflow`),
  retryWorkflow: (id: string, task: string, start_seq: number) => request('POST', `${teamUrl(id)}/workflow/retry`, { task, start_seq }),
  decideWorkflow: (id: string, body: { task: string; outcome: string; note: string; start_seq: number; log: boolean }) => request<{ decided: string }>('POST', `${teamUrl(id)}/workflow/decide`, body),
  scheduleRuns: (id: string) => get<import('./generated/ScheduleRun').ScheduleRun[]>(`${teamUrl(id)}/schedules`),
  resumeGuard: (id: string, start_seq: number) => request<{ resumed: boolean }>('POST', `${teamUrl(id)}/workflow/guard/resume`, { start_seq }),
  decisions: (id: string) => get<import('./generated/DecisionEntry').DecisionEntry[]>(`${teamUrl(id)}/decisions`),
  addDecision: (id: string, text: string) => request<{ id: string }>('POST', `${teamUrl(id)}/decisions`, { text }),
  hideDecision: (id: string, entry: string) => request<{ hidden: string }>('POST', `${teamUrl(id)}/decisions/${encodeURIComponent(entry)}/hide`, {}),
  /** What the team's agents know: the project maps made at each start, and lessons and decisions. */
  knowledge: (id: string) => get<import('./knowledge').Knowledge>(`${teamUrl(id)}/knowledge`),
  confirmDecision: (id: string, entry: string) => request<{ confirmed: string }>('POST', `${teamUrl(id)}/decisions/${encodeURIComponent(entry)}/confirm`, {}),
  listRigs: () => get<RigSummary[]>('/api/rigs'),
  boot: (specPath: string, requestId = newRequestId()) =>
    post<{ rig: string; diagnostics: Diagnostic[] }>('/api/rigs', requestId, { spec_path: specPath }),
  stopRig: (name: string, requestId = newRequestId()) =>
    post<StopRigOutcome>(`${rigUrl(name)}/stop`, requestId),
  /** Each agent's newest `tail` events for a whole team, long text shortened (one request for the board). */
  teamRecent: (id: string, tail: number) => get<Record<string, EventRecord[]>>(`${teamUrl(id)}/recent?tail=${tail}`),
  /** The agent's events after `after`, oldest first, in full. */
  seatEventsAfter: (id: string, after: number, limit: number) => get<EventRecord[]>(`${seatUrl(id)}/events?after=${after}&limit=${limit}`),
  latestSeatEvents: (id: string, tail = HISTORY_LIMIT) =>
    get<EventRecord[]>(`${seatUrl(id)}/events?tail=${tail}`),
  send: (id: string, msg: SendBody, requestId = newRequestId()) =>
    post<{ message_id: string }>(`${seatUrl(id)}/send`, requestId, msg),
  interrupt: (id: string, requestId = newRequestId()) =>
    post<Empty>(`${seatUrl(id)}/interrupt`, requestId),
  stopSeat: (id: string, requestId = newRequestId()) =>
    post<Empty>(`${seatUrl(id)}/stop`, requestId),
  /** Relaunch one agent of a running team; it then gets what it is owed. */
  restartSeat: (id: string, requestId = newRequestId()) =>
    post<Empty>(`${seatUrl(id)}/restart`, requestId),
  /** "Hand over again": pass the agent's latest finished turn to its downstream agents once. */
  handOver: (id: string, requestId = newRequestId()) =>
    post<Empty>(`${seatUrl(id)}/handover`, requestId),
  /** Re-send the agent's handoffs that never reached it. */
  resendHandoffs: (id: string, requestId = newRequestId()) =>
    post<{ resent: number }>(`${seatUrl(id)}/resend`, requestId),

  projectVisibility: () => get<import('./generated/ProjectVisibility').ProjectVisibility[]>('/api/project-visibility'),
  /** Deletes a project in the Trash forever (its design, runs and history; not its folder). */
  deleteProjectForever: (id: string) => request<{ deleted: string }>('DELETE', `/api/project-visibility/${encodeURIComponent(id)}`),
  setProjectVisibility: (id:string,status:import('./generated/ProjectVisibilityStatus').ProjectVisibilityStatus) => request<import('./generated/ProjectVisibility').ProjectVisibility>('PUT', `/api/project-visibility/${encodeURIComponent(id)}`, {status}),
  listTeams: () => get<TeamSummary[]>('/api/teams'),
  prepareTextbook: (id: string, revision: number) => request<{folder:string;files:number;commands:string[]}>('POST', `${teamUrl(id)}/textbook/prepare`, {revision}),
  templates: () => get<TemplateInfo[]>('/api/team-templates'),
  topicExecution: (id: string, body: {task:string;start_seq:number;mode:import('./generated/TopicMode').TopicMode;depends_on:string[];previous_mode:import('./generated/TopicMode').TopicMode;previous_depends_on:string[]}) => request<import('./generated/WorkflowRun').WorkflowRun>('POST', `${teamUrl(id)}/workflow/topic`, body),
  getTeam: (id: string) => get<TeamRecord>(teamUrl(id)),
  teamSpec: (id: string) => get<TeamSpec>(`${teamUrl(id)}/spec`),
  groupBlocks: () => get<import('./generated/GroupBlock').GroupBlock[]>('/api/group-blocks'),
  saveGroupBlock: (block: import('./generated/GroupBlock').GroupBlock, requestId = newRequestId()) => request<import('./generated/GroupBlock').GroupBlock>('PUT', `/api/group-blocks/${encodeURIComponent(block.id)}`, {request_id:requestId,block}),
  createTeam: (template: string, practice: boolean, name: string | null, requestId = newRequestId()) =>
    post<TeamRecord>('/api/teams', requestId, { template, practice, name }),
  saveTeam: (id: string, revision: number, draft: TeamDraft, requestId = newRequestId(), opts?: RequestOptions) =>
    request<TeamRecord>('PUT', teamUrl(id), { request_id: requestId, revision, draft }, opts),
  /** `overPlanLimit`: start although a plan the team uses is near its limit (the server's `plan_limit` refusal). */
  /** `resume`: each agent continues its last conversation where it has one. */
  startTeam: (id: string, revision: number, requestId = newRequestId(), overPlanLimit = false, resume = false) =>
    post<StartResult>(`${teamUrl(id)}/start`, requestId, { revision, ...(overPlanLimit ? { over_plan_limit: true } : {}), ...(resume ? { resume: true } : {}) }),
  /** Agents of a stopped team with a last conversation Start could resume. */
  resumable: (id: string) => get<{ agents: { seat: string; name: string }[] }>(`${teamUrl(id)}/resumable`),
  stopTeam: (id: string, requestId = newRequestId()) =>
    post<StopRigOutcome>(`${teamUrl(id)}/stop`, requestId),
  /** Stops every agent now, like Ctrl+C: interrupted at once, forced after 2 seconds. */
  stopTeamNow: (id: string, requestId = newRequestId()) =>
    post<{ stopped: number }>(`${teamUrl(id)}/stop-now`, requestId),
  importTeam: (yaml: string, requestId = newRequestId()) =>
    post<TeamImport>('/api/teams/import', requestId, { yaml }),
  /** A `sculptured.team/v1` file; `folders` keeps this computer's folder paths. */
  teamFile: (id: string, folders = false) => {
    track('export-team-file');
    return get<unknown>(`${teamUrl(id)}/file${folders ? '?folders=true' : ''}`);
  },
  /** The team as a MADL (Multi-Agent Descriptor Language) file; `folders` keeps this computer's folder paths. */
  teamMadl: (id: string, folders = false) => {
    track('export-team-file');
    return get<{ yaml: string }>(`${teamUrl(id)}/madl${folders ? '?folders=true' : ''}`);
  },
  /** Your own Claude Code / Codex proposes a change to the design; `proposal` continues from an earlier one. Nothing is saved. */
  designAssistant: (id: string, body: { options: { cli: 'claude' | 'codex'; model: string | null }; messages: AssistantTurn[]; proposal: string | null }) =>
    request<DesignProposal>('POST', `${teamUrl(id)}/assistant`, body),
  /** A MADL file (YAML or JSON) as a new team, with what Sculptured could not keep. */
  importMadl: (text: string, requestId = newRequestId()) => {
    track('import-team-file');
    return post<TeamImport>('/api/teams/import-madl', requestId, { text });
  },
  /** Download address of a run as a `sculptured.session/v1` file. */
  runFileUrl: (id: string, seq: number) => `${teamUrl(id)}/deployments/${seq}/session`,
  /** A better version of a team rebuilt from an imported session, proposed by the user's own CLI; nothing is saved. */
  improveFromRun: (id: string, startSeq: number, options: { cli: 'claude' | 'codex'; model: string | null }) =>
    request<GeneratedTeam>('POST', `${teamUrl(id)}/improve-from-run`, { options, start_seq: startSeq }),
  /** Claude Code or Codex session files as a run file, with what was found; nothing is stored yet. */
  convertSession: (files: { name: string; text: string }[]) => {
    return request<import('./structure/SessionPreview').Converted>('POST', '/api/runs/convert', { files });
  },
  importRun: (text: string) => {
    track('import-run');
    return request<{ team: TeamRecord; run: import('./generated/DeploymentRun').DeploymentRun; warnings: DraftIssue[] }>('POST', '/api/runs/import', undefined, { text: { body: text, type: 'application/x-ndjson' } });
  },
  importTeamFile: (file: unknown, requestId = newRequestId(), origin?: import('./generated/TeamOrigin').TeamOrigin) => {
    // A team drafted with Describe it (it has an origin) is not a file import.
    if (!origin) track('import-team-file');
    return post<{ team: TeamRecord; warnings: DraftIssue[] }>('/api/teams/import-file', requestId, origin ? { file, origin } : { file });
  },
  /** The import's checks for a team file, without creating anything. */
  checkTeamFile: (file: unknown) => request<{ issues: DraftIssue[] }>('POST', '/api/teams/check-file', { file }),
  /** Your own Claude Code / Codex asks 2–4 questions about the described team. */
  generateQuestions: (body: { options: GenerateOptions; prompt: string }, signal?: AbortSignal) =>
    request<{ questions: { question: string; suggestions: string[] }[] }>('POST', '/api/generate/questions', body, { signal }),
  /** Your own Claude Code / Codex drafts 1 or 3 teams, as team files with their issues. */
  generateTeams: (body: { options: GenerateOptions; prompt: string; answers: { question: string; answer: string }[]; count: 1 | 3 }, signal?: AbortSignal) =>
    request<{ alternatives: GeneratedTeam[]; cli: string; model: string | null }>('POST', '/api/generate/teams', body, { signal }),
  /** Hosted: `on.team` browses the computer that runs that team; `on.host` a given computer. */
  /** `files`: also the folder's file names (names and paths only). */
  listFolders: (path?: string, on: { team?: string; host?: string } = {}, files = false) => {
    const qs = Object.entries({ path, team: on.team, host: on.host, files: files ? 'true' : undefined })
      .filter(([, v]) => v).map(([k, v]) => `${k}=${encodeURIComponent(v!)}`).join('&');
    return get<FolderListing>(qs ? `/api/folders?${qs}` : '/api/folders');
  },
  createFolder: (parent: string, name: string, requestId = newRequestId(), on: { team?: string; host?: string } = {}) =>
    post<FolderEntry>('/api/folders', requestId, { parent, name, ...on }),

  inbox: () => request<InboxItem[]>('GET', '/api/inbox'),
  seatQuestions: (id: string) => request<QuestionView[]>('GET', `${seatUrl(id)}/questions`),
  /** Every question the team's agents asked, newest first. */
  teamQuestions: (id: string) => request<QuestionView[]>('GET', `${teamUrl(id)}/questions`),
  answer: (questionId: string, answer: Answer, requestId = newRequestId()) => {
    track('answer-question');
    return request<QuestionView>('POST', `${questionUrl(questionId)}/answer`, { request_id: requestId, answer });
  },
  /** Sends the recorded answer (or expiry) again when it may not have reached the agent (`QuestionView.unconfirmed`). */
  sendAgain: (questionId: string, requestId = newRequestId()) =>
    request<QuestionView>('POST', `${questionUrl(questionId)}/send-again`, { request_id: requestId }),
  acknowledge: (questionId: string, requestId = newRequestId()) =>
    request<QuestionView>('POST', `${questionUrl(questionId)}/acknowledge`, { request_id: requestId }),
  machineMetrics: () => request<import('./generated/MachineSample').MachineSample[]>('GET','/api/system/metrics'),
  gitStatus: (id:string) => request<import('./generated/GitSnapshot').GitSnapshot>('GET',`${teamUrl(id)}/git`),
  gitDiff: (id:string,path:string,staged:boolean) => request<{text:string}>('GET',`${teamUrl(id)}/git/diff?path=${encodeURIComponent(path)}&staged=${staged}`),
  gitAction: (id:string,action:string,fingerprint:string,paths:string[]=[],message='') => request<import('./generated/GitSnapshot').GitSnapshot>('POST',`${teamUrl(id)}/git/action`,{request_id:newRequestId(),action,fingerprint,paths,message}),
  settings: () => request<Settings>('GET', '/api/settings'),
  saveSettings: (settings: Settings, requestId = newRequestId()) =>
    request<Settings>('PUT', '/api/settings', { request_id: requestId, settings }),
  runCheck: (seat: string) => request<null>('POST', `/api/seats/${encodeURIComponent(seat)}/check`, {}),
  copies: (id: string) => request<import('./generated/AgentCopies').AgentCopies>('GET', `${teamUrl(id)}/copies`),
  copyAction: (id: string, action: 'merge' | 'remove', seat: string, requestId = newRequestId()) =>
    request<import('./generated/AgentCopies').AgentCopies>('POST', `${teamUrl(id)}/copies/${action}`, { request_id: requestId, seat }),
  teamSettings: (id: string) => request<TeamSettings>('GET', `${teamUrl(id)}/settings`),
  traces: (id: string) => request<TraceExportView>('GET', `${teamUrl(id)}/traces`),
  saveTraces: (id: string, t: SaveTraces, requestId = newRequestId()) => request<TraceExportView>('PUT', `${teamUrl(id)}/traces`, { request_id: requestId, ...t }),
  saveTeamSettings: (id: string, settings: TeamSettings, requestId = newRequestId()) =>
    request<TeamSettings>('PUT', `${teamUrl(id)}/settings`, { request_id: requestId, settings }),
  restore: (seat: string, decision: RestoreDecision, requestId = newRequestId()) =>
    request<Empty>('POST', `${seatUrl(seat)}/restore`, { request_id: requestId, decision }),
  /** The team's one answer to "Resume N agents?" after a restart (decision 1): applies to every agent waiting. */
  restoreTeam: (team: string, decision: RestoreDecision, requestId = newRequestId()) =>
    request<Empty>('POST', `${teamUrl(team)}/restore`, { request_id: requestId, decision }),
};
