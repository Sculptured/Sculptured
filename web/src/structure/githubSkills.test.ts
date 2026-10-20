import { afterEach, expect, it, vi } from 'vitest';
import { loadCatalog, repoOf } from './githubSkills';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(typeof body === 'string' ? body : JSON.stringify(body), { status }));
const md = (name: string, description: string) => `---\nname: ${name}\ndescription: ${description}\n---\n\n# ${name}\nDo it well.\n`;

it('reads a repository given as owner/name or a GitHub address', () => {
  expect(repoOf('google/skills')).toBe('google/skills');
  expect(repoOf(' https://github.com/anthropics/skills/tree/main ')).toBe('anthropics/skills');
  expect(repoOf('not a repo')).toBeNull();
  expect(repoOf('https://example.com/a/b')).toBeNull();
});

it("lists a repository's skills at one commit and counts the ones it cannot read", async () => {
  const urls: string[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((u) => {
    const url = String(u);
    urls.push(url);
    if (url === 'https://api.github.com/repos/google/skills/commits/HEAD') return reply({ sha: 'abc1234def' });
    if (url === 'https://api.github.com/repos/google/skills/git/trees/abc1234def?recursive=1')
      return reply({ tree: [{ path: 'skills/bigquery/SKILL.md', type: 'blob' }, { path: 'skills/broken/SKILL.md', type: 'blob' }, { path: 'README.md', type: 'blob' }] });
    if (url === 'https://raw.githubusercontent.com/google/skills/abc1234def/skills/bigquery/SKILL.md') return reply(md('bigquery', 'Use when querying BigQuery.'));
    if (url === 'https://raw.githubusercontent.com/google/skills/abc1234def/skills/broken/SKILL.md') return reply('no front matter');
    return reply({}, 404);
  });
  const c = await loadCatalog('google/skills');
  expect(c.commit).toBe('abc1234def');
  expect(c.skills.map((s) => [s.path, s.skill.name, s.skill.description])).toEqual([['skills/bigquery/SKILL.md', 'bigquery', 'Use when querying BigQuery.']]);
  expect(c.skills[0].skill.source).toBe('https://github.com/google/skills/blob/abc1234def/skills/bigquery/SKILL.md');
  expect(c.unreadable).toBe(1);
  expect(urls.every((u) => u.startsWith('https://api.github.com/') || u.startsWith('https://raw.githubusercontent.com/'))).toBe(true);
});

it('says plainly when GitHub refuses or the repository is missing', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation(() => reply({ message: 'Not Found' }, 404));
  await expect(loadCatalog('nobody/nothing')).rejects.toThrow('GitHub has no repository nobody/nothing');
  vi.mocked(globalThis.fetch).mockImplementation(() => reply({ message: 'API rate limit exceeded' }, 403));
  await expect(loadCatalog('google/skills')).rejects.toThrow(/GitHub is limiting requests/);
});
