import type { Skill } from '../generated/Skill';
import { parseSkillMd } from './skills';

/** A GitHub repository of Agent Skills (folders with a `SKILL.md`), read at one commit. */
export type SkillCatalogRead = { repo: string; commit: string; skills: { path: string; skill: Skill }[]; unreadable: number };

/** Catalogs offered first; any other GitHub repository of skills works the same way. */
export const KNOWN_CATALOGS = [
  { repo: 'anthropics/skills', name: 'Anthropic' },
  { repo: 'google/skills', name: 'Google' },
];

/** Most skills read from one repository. */
const MAX_SKILLS = 200;

/** `owner/name` from `owner/name` or a github.com address; null when it is neither. */
export function repoOf(text: string): string | null {
  const t = text.trim().replace(/\.git$/, '');
  const m = /^(?:https:\/\/github\.com\/)?([A-Za-z0-9-]+)\/([A-Za-z0-9._-]+)(?:\/.*)?$/.exec(t);
  return m ? `${m[1]}/${m[2]}` : null;
}

async function github<T>(repo: string, path: string): Promise<T> {
  const res = await fetch(`https://api.github.com/repos/${repo}/${path}`, { headers: { accept: 'application/vnd.github+json' } });
  if (res.status === 404) throw new Error(`GitHub has no repository ${repo}`);
  if (res.status === 403 || res.status === 429) throw new Error('GitHub is limiting requests from this address. Try again in a few minutes.');
  if (!res.ok) throw new Error(`GitHub answered ${res.status} for ${repo}`);
  return (await res.json()) as T;
}

/** Every readable skill in the repository at its default branch's newest commit. */
export async function loadCatalog(repo: string): Promise<SkillCatalogRead> {
  const { sha } = await github<{ sha: string }>(repo, 'commits/HEAD');
  const { tree } = await github<{ tree: { path: string; type: string }[] }>(repo, `git/trees/${sha}?recursive=1`);
  const paths = tree.filter((t) => t.type === 'blob' && /(^|\/)SKILL\.md$/.test(t.path)).map((t) => t.path).slice(0, MAX_SKILLS);
  const read = await Promise.all(paths.map(async (path) => {
    const res = await fetch(`https://raw.githubusercontent.com/${repo}/${sha}/${path}`);
    const parsed = res.ok ? parseSkillMd(await res.text()) : 'unreadable';
    return typeof parsed === 'string' ? null : { path, skill: parsed };
  }));
  const skills = read.filter((s): s is { path: string; skill: Skill } => s !== null).sort((a, b) => a.skill.name.localeCompare(b.skill.name));
  return { repo, commit: sha, skills, unreadable: paths.length - skills.length };
}
