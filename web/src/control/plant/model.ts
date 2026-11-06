/**
 * The plant model behind the control-plane panes (process view, action timeline, trends): the
 * systems a team acts on, every action an agent took on one, the permits (interlocks) that gate
 * them, and each model call's usage. Both a deployment and an Arena run are read into this one
 * shape, so production and staging look the same. Pure types and small helpers.
 */

/** Whether an action read or wrote. `unknown` when nothing declares or shows it (never guessed). */
export type Effect = 'read' | 'write' | 'unknown';

/**
 * Where an effect or a system's kind came from: `declared` by the system itself (an Arena part's
 * describe, a tool's `effect`), `observed` (rows the call actually wrote), `sculptured` (a tool
 * Sculptured runs or classifies itself), or `unknown`.
 */
export type Basis = 'declared' | 'observed' | 'sculptured' | 'unknown';

/** An action's outcome. A refusal (a gate or the tool said no) and an error both stand out. */
export type Outcome = 'ok' | 'refused' | 'error' | 'running';

/** One system the team acts on, as equipment on the process view. */
export type SystemUnit = {
  id: string;
  /** "erp", "database", "api", "mcp", "messaging", "approvals", "plant", "lims", "files", "logistics", …; "unknown". */
  kind: string;
  /** The name shown: the product ("ERPNext"), or what Sculptured calls it ("Agent folders"). */
  name: string;
  /** The Arena part or MCP server it is, when that differs from the name. */
  part?: string;
  product?: string;
  /** For an open-source stand-in: the commercial systems it represents. */
  standsInFor?: string;
  /** True for a simulation, false for the real product; absent when not said. */
  simulated?: boolean;
  basis: Basis;
};

/** A permit: a change or tool use that waits on (or got) a decision. */
export type Permit = {
  id: string;
  at: number;
  agent?: string;
  title: string;
  state: 'waiting' | 'granted' | 'refused';
  /** Who decided, in words ("you", "the team's rules", "an agent"); absent while waiting or unknown. */
  by?: string;
  /** The system it is a change to, when the request says. */
  system?: string;
};

export type PlantAction = {
  id: string;
  at: number;
  /** How long it took, when recorded. */
  ms?: number;
  agent: string;
  /** The system unit's id. */
  system: string;
  effect: Effect;
  effectBasis: Basis;
  outcome: Outcome;
  /** The tool's own name. */
  tool: string;
  /** For a message or handoff: the agent it went to ("*" for everyone). */
  to?: string;
  /** Why it was refused or failed (the system's own words: evidence). */
  reason?: string;
  /** L4: the request and the result as recorded (evidence). */
  request?: unknown;
  result?: unknown;
  /** The permit that gated it, when one did. */
  permit?: string;
};

/** One model call's (or turn's) usage, for the trends. Absent fields were not reported. */
export type UsagePoint = { at: number; agent?: string; context?: number; tokens?: number; costUsd?: number };

/** What an agent may decide and do, in plain words (its interlocks). */
export type Rights = Record<string, string[]>;

export type Plant = {
  systems: SystemUnit[];
  actions: PlantAction[];
  permits: Permit[];
  rights: Rights;
  usage: UsagePoint[];
};

export const EMPTY_PLANT: Plant = { systems: [], actions: [], permits: [], rights: {}, usage: [] };

/** Short codes for the kinds a system can say it is; anything else is shown upper-cased, at most 5 letters. */
const CODES: Record<string, string> = {
  erp: 'ERP', crm: 'CRM', database: 'DB', api: 'API', mcp: 'MCP', messaging: 'MSG', approvals: 'APR', plant: 'PLANT',
  lims: 'LIMS', files: 'FILES', logistics: 'LOG', shell: 'SHELL', skills: 'SKILL', qms: 'QMS', mes: 'MES', unknown: '?',
};

export const kindCode = (kind: string) => CODES[kind] ?? kind.slice(0, 5).toUpperCase();

/** Plain words for a kind, for labels and lists. */
const WORDS: Record<string, string> = {
  erp: 'ERP', crm: 'CRM', database: 'database', api: 'HTTP API', mcp: 'MCP server', messaging: 'messaging', approvals: 'approvals',
  plant: 'plant', lims: 'LIMS', files: 'files', logistics: 'logistics', shell: 'shell', skills: 'skills', unknown: 'kind unknown',
};
export const kindWords = (kind: string) => WORDS[kind] ?? kind;

/** "R", "W" or "?" for an effect. */
export const effectCode = (e: Effect) => (e === 'read' ? 'R' : e === 'write' ? 'W' : '?');

/** An outcome as a glyph and a word (never colour alone). */
export const OUTCOME_WORDS: Record<Outcome, { glyph: string; word: string }> = {
  ok: { glyph: '✓', word: 'ok' },
  refused: { glyph: '⊘', word: 'refused' },
  error: { glyph: '✕', word: 'error' },
  running: { glyph: '◌', word: 'running' },
};

export const isAbnormal = (o: Outcome) => o === 'refused' || o === 'error';

/** What a system unit is, in one line: "ERPNext · stands in for SAP S/4HANA · simulated". */
export function systemLine(s: SystemUnit): string {
  const parts = [s.product && s.product !== s.name ? s.product : null, s.standsInFor ? `stands in for ${s.standsInFor}` : null];
  if (s.simulated === true) parts.push('simulated');
  if (s.simulated === false && s.basis === 'declared') parts.push('real');
  return parts.filter(Boolean).join(' · ');
}
