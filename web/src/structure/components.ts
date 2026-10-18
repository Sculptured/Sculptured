import type { Component } from '../generated/Component';
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

/** The command it runs, as people read it before they approve it. */
export const commandLine = (c: Component) => [c.program, ...(c.args ?? [])].join(' ');

/** The names of an agent's components, for its card. */
export const agentComponentNames = (d: TeamDraft | null | undefined, a: DraftAgent) =>
  (a.advanced.integrations ?? []).map((id) => componentById(d, id)?.name ?? id);

const kebab = (s: string, max: number) => s.length > 0 && s.length <= max && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(s);

/** What is wrong with a manifest, in plain words (the same rules as the server's). */
export function componentProblems(c: Component, team: Component[] = []): string[] {
  const out: string[] = [];
  if (!kebab(c.id ?? '', 40)) out.push('The id must be 1–40 lowercase letters, digits and hyphens.');
  if (BUILTIN.some((b) => b.id === c.id)) out.push(`"${c.id}" is a built-in component's id; give yours another one.`);
  if (team.some((t) => t.id === c.id)) out.push(`This team already has a component "${c.id}".`);
  if (!c.name?.trim() || c.name.length > 60) out.push('It needs a name of at most 60 characters.');
  if ((c.description ?? '').length > 500 || !c.install?.trim() || c.install.length > 500) out.push('The description and install steps are at most 500 characters, and install steps are required.');
  if (!/^[A-Za-z0-9_+-][A-Za-z0-9._+-]{0,79}$/.test(c.program ?? '')) out.push('The program must be a plain program name such as kin-mcp, with no folder.');
  if (c.kind !== 'mcp' && c.kind !== 'tool-hook') out.push('kind is "mcp" or "tool-hook".');
  if (c.kind === 'tool-hook' && !c.claude_only) out.push('A tool hook runs only for Claude Code agents; set claude_only.');
  if ((c.args ?? []).length > 32) out.push('At most 32 arguments.');
  if (c.homepage && !/^https?:\/\//.test(c.homepage)) out.push('The homepage must be a web address.');
  return out;
}

/** A manifest from pasted or loaded JSON, or why it is not one. */
export function parseManifest(text: string): Component | string {
  try {
    const v = JSON.parse(text) as unknown;
    const c = (v && typeof v === 'object' && 'component' in v ? (v as { component: unknown }).component : v) as Component;
    if (!c || typeof c !== 'object' || typeof c.id !== 'string') return 'That is not a component manifest: it needs at least an id, a name, a kind, a program and install steps.';
    return c;
  } catch {
    return 'That is not valid JSON.';
  }
}
