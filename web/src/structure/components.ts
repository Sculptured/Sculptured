import type { Component } from '../generated/Component';
import type { ComponentKind } from '../generated/ComponentKind';
import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';
import KINDEX from './components/kindex.json';
import SIGNET from './components/signet.json';

/** Sculptured's built-in components (the same files the server reads). */
export const BUILTIN: Component[] = [KINDEX as Component, SIGNET as Component];

export const isBuiltin = (c: Component) => BUILTIN.some((b) => JSON.stringify(b) === JSON.stringify(c));

/** Every component this team can give its agents: built in, then its own. */
export const availableComponents = (d: TeamDraft | null | undefined): Component[] => [...BUILTIN, ...(d?.components ?? [])];

export const componentById = (d: TeamDraft | null | undefined, id: string) => availableComponents(d).find((c) => c.id === id);

/** What it runs or connects to, as people read it before they approve it (the server's words). */
export function commandLine(c: Component): string {
  switch (c.kind) {
    case 'mcp-remote': {
      const headers = Object.entries(c.headers ?? {}).map(([k, v]) => `${k}: ${v}`);
      return `connects to ${c.url ?? ''}${headers.length ? ` with ${headers.join(', ')}` : ''}`;
    }
    case 'skills': return `adds skills ${(c.skills ?? []).map((s) => s.name).join(', ')}`;
    case 'claude-plugin': return `loads the Claude plugin ${c.plugin ?? ''}`;
    default: return [c.program ?? '', ...(c.args ?? [])].join(' ');
  }
}

/** The secrets named as `${NAME}` in a remote server's address and headers, each once. */
export function secretsOf(c: Component): string[] {
  const out: string[] = [];
  for (const text of [c.url ?? '', ...Object.values(c.headers ?? {})]) {
    for (const m of text.matchAll(/\$\{([A-Z_][A-Z0-9_]{0,63})\}/g)) if (!out.includes(m[1])) out.push(m[1]);
  }
  return out;
}

/** How each kind is named to people. */
export const KIND_LABEL: Record<ComponentKind, string> = {
  mcp: 'MCP server on the computer',
  'mcp-remote': 'Remote MCP server',
  skills: 'Skill pack',
  'claude-plugin': 'Claude plugin',
  'tool-hook': 'Tool hook',
};

/** The picker's sections, in order. */
export const KIND_GROUPS: { title: string; kinds: ComponentKind[] }[] = [
  { title: 'MCP servers', kinds: ['mcp', 'mcp-remote'] },
  { title: 'Skill packs', kinds: ['skills'] },
  { title: 'Plugins and hooks', kinds: ['claude-plugin', 'tool-hook'] },
];

/** Where it acts, in a line: on the computer, at a web address, or nowhere (skills). */
export function whereItRuns(c: Component): string {
  const secrets = secretsOf(c);
  switch (c.kind) {
    case 'skills': return `Adds ${c.skills?.length ?? 0} skill${c.skills?.length === 1 ? '' : 's'} to the agent; nothing runs.`;
    case 'mcp-remote': return `${commandLine(c)}${secrets.length ? `. Set ${secrets.join(', ')} on the agent's computer: sculptured set-secret ${secrets[0]}` : ''}`;
    case 'claude-plugin': return `${commandLine(c)} from the agent's computer`;
    default: return `Runs ${commandLine(c)} on the agent's computer`;
  }
}

/** The names of an agent's components, for its card. */
export const agentComponentNames = (d: TeamDraft | null | undefined, a: DraftAgent) =>
  (a.advanced.integrations ?? []).map((id) => componentById(d, id)?.name ?? id);

const kebab = (s: string, max: number) => s.length > 0 && s.length <= max && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(s);

const PLACEHOLDER = /\$\{([^}]*)\}/g;
const placeholdersOk = (t: string) => !/\$\{[^}]*$/.test(t) && [...t.matchAll(PLACEHOLDER)].every((m) => /^[A-Z_][A-Z0-9_]{0,63}$/.test(m[1]));

/** A remote server's address and headers: https, and secrets named as ${NAME}, never written in. */
function remoteProblems(c: Component): string[] {
  const out: string[] = [];
  if ((c.kind === 'mcp-remote') !== Boolean(c.url)) out.push('A remote MCP server has a url, and only it does.');
  if (c.url) {
    if (c.url.length > 500 || !/^(https:\/\/\S+|http:\/\/(localhost|127\.0\.0\.1|\[::1\])([:/]\S*)?)$/.test(c.url)) out.push('The url must start with https:// (http:// only for a server on the agent\'s own computer).');
    if (!placeholdersOk(c.url)) out.push('A ${…} must name a secret in capitals, such as ${OPENFOAM_TOKEN}.');
  }
  for (const [name, value] of Object.entries(c.headers ?? {})) {
    if (!/^[A-Za-z0-9-]{1,64}$/.test(name)) out.push(`Header "${name}" must be letters, digits and hyphens.`);
    if (!placeholdersOk(value)) out.push('A ${…} must name a secret in capitals, such as ${OPENFOAM_TOKEN}.');
    else if (![...value.matchAll(PLACEHOLDER)].length) out.push(`Header ${name} must name its secret as \${NAME} (for example "Bearer \${API_TOKEN}"), so the secret stays on the agent's computer.`);
  }
  return out;
}

type Rule = (c: Component, team: Component[]) => string | false | undefined;

const KINDS = Object.keys(KIND_LABEL);
const runsProgram = (c: Component) => c.kind === 'mcp' || c.kind === 'tool-hook';
const PROGRAM_NAME = /^[A-Za-z0-9_+-][A-Za-z0-9._+-]{0,79}$/;
const PLUGIN_NAME = /^[A-Za-z0-9_-][A-Za-z0-9._-]*@[A-Za-z0-9_-][A-Za-z0-9._-]*$/;
const tooLong = (v: string | undefined, max: number) => (v ?? '').length > max;
const pluginMisnamed = (c: Component) => !PLUGIN_NAME.test(c.plugin ?? '') || tooLong(c.plugin, 120);

/** Its id, name, kind, description and install steps. */
const IDENTITY: Rule[] = [
  (c) => !kebab(c.id ?? '', 40) && 'The id must be 1–40 lowercase letters, digits and hyphens.',
  (c) => BUILTIN.some((b) => b.id === c.id) && `"${c.id}" is a built-in component's id; give yours another one.`,
  (c, team) => team.some((t) => t.id === c.id) && `This team already has a component "${c.id}".`,
  (c) => (!c.name?.trim() || c.name.length > 60) && 'It needs a name of at most 60 characters.',
  (c) => !KINDS.includes(c.kind) && `kind is one of ${KINDS.map((k) => `"${k}"`).join(', ')}.`,
  (c) => (tooLong(c.description, 500) || tooLong(c.install, 500)) && 'The description and install steps are at most 500 characters.',
  (c) => c.kind !== 'skills' && !c.install?.trim() && 'Say how to set it up on a computer (the install steps).',
];

/** The program it runs, if any, and what runs it. */
const PROGRAM: Rule[] = [
  (c) => runsProgram(c) && !PROGRAM_NAME.test(c.program ?? '') && 'The program must be a plain program name such as kin-mcp, with no folder.',
  (c) => !runsProgram(c) && !!(c.program || c.args?.length) && 'Only an MCP server on the computer or a tool hook runs a program.',
  (c) => (c.kind === 'tool-hook' || c.kind === 'claude-plugin') && !c.claude_only && 'Tool hooks and Claude plugins work only for Claude Code agents; set claude_only.',
  (c) => (c.args ?? []).length > 32 && 'At most 32 arguments.',
];

/** Skill packs, Claude plugins and the homepage. */
const PACKAGE: Rule[] = [
  (c) => (c.kind === 'skills') !== Boolean(c.skills?.length) && 'A skill pack carries one or more skills, and only it does.',
  (c) => (c.skills ?? []).length > 20 && 'At most 20 skills in one pack.',
  (c) => ((c.kind === 'claude-plugin') !== Boolean(c.plugin) || (!!c.plugin && pluginMisnamed(c))) && 'A Claude plugin is named as name@marketplace, as `claude plugin install` takes it.',
  (c) => !!c.homepage && !/^https?:\/\//.test(c.homepage) && 'The homepage must be a web address.',
];

const check = (rules: Rule[], c: Component, team: Component[]) => rules.map((r) => r(c, team)).filter((m): m is string => typeof m === 'string');

/** What is wrong with a manifest, in plain words (the same rules as the server's). */
export function componentProblems(c: Component, team: Component[] = []): string[] {
  return [...new Set([...check(IDENTITY, c, team), ...check(PROGRAM, c, team), ...remoteProblems(c), ...check(PACKAGE, c, team)])];
}

/** A manifest from pasted or loaded JSON, or why it is not one. */
export function parseManifest(text: string): Component | string {
  try {
    const v = JSON.parse(text) as unknown;
    const c = (v && typeof v === 'object' && 'component' in v ? (v as { component: unknown }).component : v) as Component;
    if (!c || typeof c !== 'object' || typeof c.id !== 'string') return 'That is not a component manifest: it needs at least an id, a name and a kind.';
    return c;
  } catch {
    return 'That is not valid JSON.';
  }
}
