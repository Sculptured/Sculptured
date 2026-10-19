import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { draftAgent, teamDraft } from '../test-fixtures';
import type { TeamDraft } from '../generated/TeamDraft';
import { ImproveFromRun, diffTeams } from './ImproveFromRun';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));

const current = teamDraft({
  id: 'login', agents: [draftAgent('main', { name: 'Login page', job: 'Add a login page' }), draftAgent('sub-1', { name: 'Explore auth', job: 'Find how auth works' })],
  connections: [{ id: 'c-1', from: 'main', to: 'sub-1', kind: 'hands-work-to' }],
});
const proposed = teamDraft({
  agents: [draftAgent('lead', { name: 'Login page', job: 'Lead the login work' }), draftAgent('researcher', { name: 'Researcher', job: 'Study the code first' })],
  connections: [{ id: 'c-1', from: 'lead', to: 'researcher', kind: 'hands-work-to' }],
});

function serve(runs: { start_seq: number; source: string | null }[]) {
  const posts: { url: string; body: unknown }[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (init?.method === 'POST') posts.push({ url: u, body: JSON.parse(String(init.body)) });
    if (u.endsWith('/deployments')) return reply(runs.map((r) => ({ ...r, started_ms: 0, initial_agents: 2 })));
    if (u.endsWith('/api/clis')) return reply([{ cli: 'claude', installed: true, signed_in: true }]);
    if (u.endsWith('/improve-from-run')) return reply({ label: 'team', rationale: 'A lead and a researcher.', draft: proposed, dropped: [], issues: [] });
    return reply({});
  });
  return posts;
}

it('lists what the AI would change, each labelled AI-reconstructed', () => {
  const rows = diffTeams(current, proposed);
  expect(rows.map((r) => `${r.change} ${r.what}`)).toEqual([
    'changed Login page', 'added Researcher', 'removed Explore auth', 'added arrow Login page → Researcher', 'removed arrow Login page → Explore auth',
  ]);
});

it('is offered only for a team with an imported session', async () => {
  serve([{ start_seq: 4, source: 'sculptured' }]);
  const { container } = render(<ImproveFromRun teamId="login" draft={current} edit={vi.fn()} />);
  await new Promise((r) => setTimeout(r, 20));
  expect(container).toBeEmptyDOMElement();
});

it('asks the CLI, shows the diff, and applies it to the design', async () => {
  const posts = serve([{ start_seq: -3, source: 'imported:claude-code' }]);
  let saved: TeamDraft | null = null;
  const edit = vi.fn((fn: (d: TeamDraft) => TeamDraft) => { saved = fn(current); return null; });
  render(<ImproveFromRun teamId="login" draft={current} edit={edit as never} />);
  await userEvent.click(await screen.findByRole('button', { name: 'Improve this team with AI' }));
  expect(screen.getByText(/reads a summary of the session \(no file contents\)/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: /^Ask/ }));
  expect(await screen.findByText('A lead and a researcher.')).toBeInTheDocument();
  expect(screen.getAllByText('AI-reconstructed')).toHaveLength(5);
  expect(posts.find((p) => p.url.endsWith('/improve-from-run'))?.body).toEqual(expect.objectContaining({ start_seq: -3 }));
  await userEvent.click(screen.getByRole('button', { name: 'Apply' }));
  expect(saved!.agents.map((a) => a.name)).toEqual(['Login page', 'Researcher']);
  expect(saved!.id).toBe('login');
});

it('discard leaves the design as it was', async () => {
  serve([{ start_seq: -3, source: 'imported:codex' }]);
  const edit = vi.fn();
  render(<ImproveFromRun teamId="login" draft={current} edit={edit} />);
  await userEvent.click(await screen.findByRole('button', { name: 'Improve this team with AI' }));
  await userEvent.click(screen.getByRole('button', { name: /^Ask/ }));
  await userEvent.click(await screen.findByRole('button', { name: 'Discard' }));
  expect(edit).not.toHaveBeenCalled();
  expect(screen.queryByText('A lead and a researcher.')).toBeNull();
});
