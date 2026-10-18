import type { StateKey } from '../herdr';
import type { Command } from './commands';
import { fuzzyScore } from './fuzzy';
import type { AgentRow, TeamRow } from './model';

export type Filter = 'blocked' | 'working' | 'idle' | 'done';
export const FILTERS: { key: Filter; alt: string; label: string; match: (k: StateKey) => boolean }[] = [
  { key: 'blocked', alt: 'b', label: 'blocked', match: (k) => k === 'needs-you' || k === 'failed' },
  { key: 'working', alt: 'w', label: 'working', match: (k) => k === 'working' },
  { key: 'idle', alt: 'i', label: 'idle', match: (k) => k === 'idle' },
  { key: 'done', alt: 'd', label: 'done', match: (k) => k === 'done' },
];

/** What the palette lists: everything, only agents (`@`, or opened with g), or only commands (`>`). */
export type Scope = 'all' | 'agents' | 'commands';

export type Row = { kind: 'cmd'; cmd: Command; key: string } | { kind: 'agent'; a: AgentRow; key: string };
/** `team`: a team's agents, so rows show their group rather than their team. */
export type Section = { label: string; rows: Row[]; team?: boolean };

export const cmdRow = (cmd: Command): Row => ({ kind: 'cmd', cmd, key: `cmd:${cmd.id}` });
export const agentRow = (a: AgentRow): Row => ({ kind: 'agent', a, key: `agent:${a.id}` });

/** How many of each the short list shows before you type. */
const SHORT = { recent: 4, suggested: 6, agents: 6 };

/** Every whitespace-separated term must appear in the agent's name, team or folder. */
export function matches(a: AgentRow, query: string): boolean {
  const hay = `${a.name} ${a.rig} ${a.folder} ${a.group}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((t) => hay.includes(t));
}

export type PaletteInput = {
  scope: Scope;
  teams: TeamRow[];
  commands: Command[];
  query: string;
  filters: Set<Filter>;
  /** Keys of what ran last, newest first. */
  recent: string[];
  /** The open project, whose agents the short list shows. */
  project?: string;
};

/** The palette's rows, in sections. */
export function buildSections(p: PaletteInput): Section[] {
  const active = FILTERS.filter((f) => p.filters.has(f.key));
  const agentOk = (a: AgentRow) => matches(a, p.query) && (active.length === 0 || active.some((f) => f.match(a.state.key)));
  const q = p.query.trim();
  if (p.scope === 'agents' || active.length > 0) return byTeam(p.teams, agentOk);
  const scored = p.commands
    .map((cmd) => ({ cmd, score: q ? fuzzyScore(q, `${cmd.title} ${cmd.section} ${cmd.words ?? ''}`) : 0 }))
    .filter((x): x is Scored => x.score !== null);
  if (p.scope === 'commands') return q ? nonEmpty([{ label: 'Commands', rows: rank(scored).map(cmdRow) }]) : bySection(scored.map((x) => x.cmd));
  if (!q) return shortList(p);
  const agents = p.teams
    .flatMap((t) => t.agents)
    .filter(agentOk)
    .map((a) => ({ a, score: fuzzyScore(q, a.name) ?? 0 }))
    .sort((x, y) => y.score - x.score)
    .slice(0, 30);
  return nonEmpty([
    { label: 'Commands', rows: rank(scored).slice(0, 12).map(cmdRow) },
    { label: 'Agents', rows: agents.map((x) => agentRow(x.a)) },
  ]);
}

type Scored = { cmd: Command; score: number };
const nonEmpty = (xs: Section[]) => xs.filter((s) => s.rows.length > 0);

/** Best match first; a command that cannot run now goes below those that can. */
function rank(xs: Scored[]): Command[] {
  const value = (x: Scored) => x.score - (x.cmd.disabled ? 50 : 0);
  return [...xs].sort((a, b) => value(b) - value(a)).map((x) => x.cmd);
}

function byTeam(teams: TeamRow[], ok: (a: AgentRow) => boolean): Section[] {
  return nonEmpty(teams.map((t) => ({ label: t.rig, team: true, rows: t.agents.filter(ok).map(agentRow) })));
}

function bySection(commands: Command[]): Section[] {
  const out = new Map<string, Row[]>();
  for (const cmd of commands) out.set(cmd.section, [...(out.get(cmd.section) ?? []), cmdRow(cmd)]);
  return [...out].map(([label, rows]) => ({ label, rows }));
}

/** Before you type: what you ran recently, the likely next steps here, and the open project's agents. */
function shortList(p: PaletteInput): Section[] {
  const byKey = new Map<string, Row>([...p.commands.map(cmdRow), ...p.teams.flatMap((t) => t.agents).map(agentRow)].map((r) => [r.key, r]));
  const recent = p.recent
    .map((k) => byKey.get(k))
    .filter((r): r is Row => !!r && !(r.kind === 'cmd' && r.cmd.disabled))
    .slice(0, SHORT.recent);
  const seen = new Set(recent.map((r) => r.key));
  const suggested = p.commands
    .filter((cmd) => cmd.rank !== undefined && !cmd.disabled && !seen.has(`cmd:${cmd.id}`))
    .sort((a, b) => a.rank! - b.rank!)
    .slice(0, SHORT.suggested);
  const own = p.teams.find((t) => t.rig === p.project) ?? p.teams[0];
  const agents = (own?.agents ?? []).filter((a) => !seen.has(`agent:${a.id}`)).slice(0, SHORT.agents);
  return nonEmpty([
    { label: 'Recent', rows: recent },
    { label: 'Suggested', rows: suggested.map(cmdRow) },
    { label: own ? `Agents · ${own.title}` : 'Agents', rows: agents.map(agentRow), team: true },
  ]);
}
