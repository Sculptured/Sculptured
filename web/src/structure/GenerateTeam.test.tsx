import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import { GenerateTeam } from './GenerateTeam';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['clis', 'hosts', 'generateQuestions', 'generateTeams', 'checkDraft', 'createDraft'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const cli = (name: string, signed_in: boolean) => ({ name, installed: true, version: '1', signed_in, account: null, detail: '' });
const agent = (id: string, name: string, x: number) => ({ id, group_id: null, name, kind: 'claude' as const, job: `${name}'s job`, folder: null, x, y: 72, advanced: { model: null, permission: 'unrestricted' as const, resume: 'resume-if-possible' as const } });
const file = (agents: ReturnType<typeof agent>[], connections: { id: string; from: string; to: string; kind: 'hands-work-to' }[]) =>
  ({ id: 'docs', name: 'Docs', groups: [], agents, connections, revision: 0 });
const alt = (label: string, d: ReturnType<typeof file>) => ({ label, rationale: `${label} because`, draft: d, dropped: [], issues: [] });
const balanced = file([agent('writer', 'Writer', 40), agent('reviewer', 'Reviewer', 300)], [{ id: 'c-writer-reviewer', from: 'writer', to: 'reviewer', kind: 'hands-work-to' }]);

beforeEach(() => {
  vi.mocked(api.clis).mockReset().mockResolvedValue([cli('claude', true), cli('codex', false)]);
  vi.mocked(api.hosts).mockReset().mockResolvedValue([]);
  vi.mocked(api.checkDraft).mockReset().mockResolvedValue({ issues: [] });
  vi.mocked(api.generateQuestions).mockReset();
  vi.mocked(api.generateTeams).mockReset();
  vi.mocked(api.createDraft).mockReset();
});

it('asks questions, offers three teams, and creates the one picked with an agent dropped', async () => {
  const onCreated = vi.fn();
  render(<GenerateTeam onCreated={onCreated} onCancel={() => {}} />);
  const user = userEvent.setup();
  await user.type(screen.getByRole('textbox', { name: 'Describe the team' }), 'Rebuild our docs site');
  expect(await screen.findByText(/Uses your Claude Code plan/)).toBeInTheDocument();
  expect(screen.getByText(/no tools or MCP servers: it sees only this description/)).toBeInTheDocument();
  vi.mocked(api.generateQuestions).mockResolvedValue({ questions: [{ question: 'Who reviews?', suggestions: ['A reviewer agent', 'Nobody'] }] });
  await user.click(screen.getByRole('button', { name: 'Next: questions' }));
  await user.click(await screen.findByRole('radio', { name: 'A reviewer agent' }));
  vi.mocked(api.generateTeams).mockResolvedValue({
    alternatives: [alt('lean', file([agent('writer', 'Writer', 40)], [])), alt('balanced', balanced), alt('thorough', balanced)],
    cli: 'claude',
    model: null,
  });
  await user.click(screen.getByRole('button', { name: 'Generate' }));
  expect(api.generateTeams).toHaveBeenCalledWith(expect.objectContaining({ prompt: 'Rebuild our docs site', count: 3, answers: [{ question: 'Who reviews?', answer: 'A reviewer agent' }] }), expect.anything());
  await user.click(await screen.findByRole('button', { name: 'Use balanced' }));
  const reviewer = await screen.findByRole('checkbox', { name: /^Reviewer/ });
  const arrow = screen.getByRole('checkbox', { name: 'Writer hands work to Reviewer' });
  expect(arrow).toBeChecked();
  await user.click(reviewer);
  expect(arrow).not.toBeChecked();
  expect(arrow).toBeDisabled();
  vi.mocked(api.createDraft).mockResolvedValue({ team: { draft: { id: 'gen-1' } }, warnings: [] } as never);
  await user.click(screen.getByRole('button', { name: 'Create team' }));
  await waitFor(() => expect(onCreated).toHaveBeenCalledWith('gen-1'));
  const [created, , origin] = vi.mocked(api.createDraft).mock.calls[0] as unknown as [ReturnType<typeof file>, string, { cli: string; prompt: string }];
  expect(created.agents.map((a) => a.name)).toEqual(['Writer']);
  expect(created.connections).toEqual([]);
  expect(origin).toMatchObject({ kind: 'generated', cli: 'claude', prompt: 'Rebuild our docs site' });
});

it('can skip the questions and ask for one team', async () => {
  render(<GenerateTeam onCreated={() => {}} onCancel={() => {}} />);
  const user = userEvent.setup();
  await user.type(screen.getByRole('textbox', { name: 'Describe the team' }), 'A docs team');
  await user.click(screen.getByText('Options'));
  await user.click(screen.getByRole('checkbox', { name: 'Ask me questions first' }));
  await user.selectOptions(screen.getByRole('combobox', { name: 'Alternatives' }), '1');
  vi.mocked(api.generateTeams).mockResolvedValue({ alternatives: [alt('team', balanced)], cli: 'claude', model: null });
  await user.click(screen.getByRole('button', { name: 'Generate' }));
  expect(api.generateQuestions).not.toHaveBeenCalled();
  expect(api.generateTeams).toHaveBeenCalledWith(expect.objectContaining({ count: 1, options: expect.objectContaining({ kinds: 'claude' }) }), expect.anything());
  expect(await screen.findByRole('button', { name: 'Create team' })).toBeInTheDocument();
});

it("shows what the CLI answered when it can't be used", async () => {
  render(<GenerateTeam onCreated={() => {}} onCancel={() => {}} />);
  const user = userEvent.setup();
  await user.type(screen.getByRole('textbox', { name: 'Describe the team' }), 'A docs team');
  await user.click(screen.getByText('Options'));
  await user.click(screen.getByRole('checkbox', { name: 'Ask me questions first' }));
  vi.mocked(api.generateTeams).mockRejectedValue(new ApiError(502, 'generate_unusable', "Claude Code's answer could not be used: no agents", undefined, undefined, 'RAW ANSWER'));
  await user.click(screen.getByRole('button', { name: 'Generate' }));
  const alert = await screen.findByRole('alert');
  expect(alert).toHaveTextContent('could not be used');
  await user.click(within(alert).getByText('What Claude Code answered'));
  expect(within(alert).getByText('RAW ANSWER')).toBeVisible();
});

it('says what Codex can see, and asks which computer when several are connected', async () => {
  vi.mocked(api.hosts).mockResolvedValue([
    { id: 'd1', drivers: [], policy: null },
    { id: 'd2', drivers: [], policy: null },
  ]);
  render(<GenerateTeam onCreated={() => {}} onCancel={() => {}} />);
  const user = userEvent.setup();
  await user.type(screen.getByRole('textbox', { name: 'Describe the team' }), 'A docs team');
  await user.click(screen.getByText('Options'));
  const computer = await screen.findByRole('combobox', { name: 'Computer' });
  await user.selectOptions(computer, 'd2');
  await waitFor(() => expect(api.clis).toHaveBeenLastCalledWith('d2'));
  await user.selectOptions(screen.getByRole('combobox', { name: 'CLI' }), 'codex');
  expect(screen.getByText(/Codex runs read-only: it can read files on this computer but not change them/)).toBeInTheDocument();
  await user.click(screen.getByRole('checkbox', { name: 'Ask me questions first' }));
  vi.mocked(api.generateTeams).mockResolvedValue({ alternatives: [alt('team', balanced)], cli: 'codex', model: null });
  await user.click(screen.getByRole('button', { name: 'Generate' }));
  expect(api.generateTeams).toHaveBeenCalledWith(expect.objectContaining({ options: expect.objectContaining({ host: 'd2', cli: 'codex', kinds: 'codex' }) }), expect.anything());
});
