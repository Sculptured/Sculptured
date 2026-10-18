import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { RigSnapshot } from '../generated/RigSnapshot';
import { compiledRig, draftAgent, FakeEventSource, seatView, teamDraft, teamRecord } from '../test-fixtures';
import { forgetViewports } from '../shell/viewport';
import { ALL_TEAMS_STREAM } from '../useTeams';
import { fakeTeam, renderAt, resetApi, where } from './harness';
import { limitNotes, startConfirmation, STOP_ANY_TIME } from './StartConfirm';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'listFolders', 'listRigs', 'latestSeatEvents', 'stopRig'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const WEB = '/home/me/projects/web-shop';
const NO_LIMITS = 'No run budget or agent limits are set: agents work until they finish or you stop them.';
const claude = (id: string, over = {}) => draftAgent(id, { kind: 'claude', folder: WEB, ...over });
/** Lead hands work to Builder and Tester; Builder hands work to Reviewer (Codex). */
const realTeam = () =>
  teamDraft({
    agents: [claude('lead', { x: 40 }), claude('builder', { x: 300 }), claude('tester', { x: 560 }), draftAgent('reviewer', { kind: 'codex', folder: '/home/me/notes', x: 820 })],
    connections: [
      { id: 'c1', from: 'lead', to: 'builder', kind: 'hands-work-to' },
      { id: 'c2', from: 'lead', to: 'tester', kind: 'hands-work-to' },
      { id: 'c3', from: 'builder', to: 'reviewer', kind: 'hands-work-to' },
    ],
  });

beforeEach(() => {
  resetApi();
  FakeEventSource.reset();
  forgetViewports();
  localStorage.clear();
});

describe('startConfirmation', () => {
  it('is null for a team of Practice agents only: those start without asking', () => {
    expect(startConfirmation(teamDraft(), '/home/me')).toBeNull();
    expect(startConfirmation(undefined, '/home/me')).toBeNull();
  });

  it('says how many real agents start, where, on whose account, and who goes first', () => {
    const d = teamDraft({ agents: [claude('lead'), claude('builder'), draftAgent('helper')], connections: [{ id: 'c', from: 'lead', to: 'builder', kind: 'hands-work-to' }] });
    expect(startConfirmation(d, '/home/me')).toEqual({
      title: 'Start 2 Claude agents?',
      // The Practice helper has no upstream either, so it starts first too.
      body: 'They run in ~/projects/web-shop and use your Claude account. First to start: Lead and Helper.',
      efficiency: [],
      limits: [NO_LIMITS],
      team: 't',
    });
  });

  it('names Codex too, every folder, and ignores connections that do not hand work over', () => {
    const d = realTeam();
    d.connections.push({ id: 'c4', from: 'reviewer', to: 'lead', kind: 'reports-to' });
    expect(startConfirmation(d, '/home/me')).toEqual({
      title: 'Start 3 Claude agents and 1 Codex agent?',
      body: 'They run in 2 folders (~/projects/web-shop and ~/notes) and use your Claude and Codex accounts. First to start: Lead.',
      efficiency: [],
      limits: [NO_LIMITS],
      team: 't',
    });
    // Without a known home folder the full path is shown.
    expect(startConfirmation(teamDraft({ agents: [claude('solo')], connections: [] }), null)?.body).toBe(
      `They run in ${WEB} and use your Claude account. First to start: Solo.`,
    );
  });
});

describe('what the confirmation says about reach and limits', () => {
  it('says plainly when agents work in the whole home folder', () => {
    const noFolder = teamDraft({ agents: [draftAgent('solo', { kind: 'claude', folder: null })], connections: [] });
    expect(startConfirmation(noFolder, '/home/me')?.broad).toMatch(/^This agent works in your whole home folder \(~\), so it can read and change any file in it\./);
    const homeItself = teamDraft({ agents: [claude('lead'), draftAgent('helper', { kind: 'codex', folder: '/home/me/' })], connections: [] });
    expect(startConfirmation(homeItself, '/home/me')?.broad).toMatch(/^Helper works in your whole home folder/);
    expect(startConfirmation(teamDraft({ agents: [claude('lead')], connections: [] }), '/home/me')?.broad).toBeUndefined();
  });

  it('names agents with their own limits, and says when nothing limits the run', () => {
    const d = teamDraft({ agents: [claude('lead', { advanced: { ...draftAgent('x').advanced, token_limit: { tokens: 1000, counts: 'all', include_subagents: true }, minute_limit: { hard: 30 } } }), claude('builder')], connections: [] });
    expect(limitNotes(d)).toEqual(['Agent limits: Lead (tokens, time). At the soft value an agent is asked to wrap up; at the hard value it is stopped.']);
    expect(limitNotes(teamDraft({ agents: [claude('lead')], connections: [] }))).toEqual([NO_LIMITS]);
  });
});

describe('Start team asks first when the team has real agents', () => {
  it('on the Structure board: Cancel starts nothing, Start team in the dialog starts it', async () => {
    fakeTeam(teamRecord({ draft: realTeam() }));
    vi.mocked(api.startTeam).mockResolvedValue({ rig: 't', warnings: [] });
    renderAt('/team/t/structure');
    const user = userEvent.setup();
    const start = await screen.findByRole('button', { name: 'Start team' });
    await waitFor(() => expect(start).toBeEnabled());
    await user.click(start);
    const dialog = screen.getByRole('alertdialog', { name: 'Start 3 Claude agents and 1 Codex agent?' });
    expect(dialog).toHaveAccessibleDescription(
      'They run in 2 folders (~/projects/web-shop and ~/notes) and use your Claude and Codex accounts. First to start: Lead.',
    );
    // Starting is never a one-way door; no agent works in the whole home folder here.
    expect(dialog).toHaveTextContent(STOP_ANY_TIME);
    expect(within(dialog).queryByText(/whole home folder/)).toBeNull();
    // The safe choice has focus, and board keys do not fire behind the dialog.
    expect(within(dialog).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await user.keyboard('g');
    expect(screen.queryByRole('dialog', { name: 'Go to agent' })).toBeNull();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(api.startTeam).not.toHaveBeenCalled();
    expect(start).toHaveFocus();

    await user.click(start);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(api.startTeam).not.toHaveBeenCalled();

    await user.click(start);
    await user.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Start team' }));
    await waitFor(() => expect(api.startTeam).toHaveBeenCalledWith('t', expect.any(Number), expect.any(String), false, false));
    await waitFor(() => expect(where()).toBe('/p/t/board?mode=run'));
  });

  it('a Practice-only team starts at once, with no dialog', async () => {
    fakeTeam();
    vi.mocked(api.startTeam).mockResolvedValue({ rig: 't', warnings: [] });
    renderAt('/team/t/structure');
    const start = await screen.findByRole('button', { name: 'Start team' });
    await waitFor(() => expect(start).toBeEnabled());
    await userEvent.setup().click(start);
    expect(screen.queryByRole('alertdialog')).toBeNull();
    await waitFor(() => expect(api.startTeam).toHaveBeenCalled());
  });

  it('on the Live board of a stopped team too', async () => {
    const draft = realTeam();
    fakeTeam(teamRecord({ draft, started_revision: 1, live: false }));
    const summary = { name: 't', desired: 'stopped' as const, seat_count: 4, running_count: 0, attention_count: 0 };
    vi.mocked(api.listRigs).mockResolvedValue([summary]);
    vi.mocked(api.listTeams).mockResolvedValue([{ id: 't', name: draft.name, agent_count: 4, updated_at_ms: 1, started: summary }]);
    vi.mocked(api.startTeam).mockResolvedValue({ rig: 't', warnings: [] });
    renderAt('/rig/t');
    await waitFor(() => expect(FakeEventSource.instances.some((i) => i.url === ALL_TEAMS_STREAM)).toBe(true));
    const base = compiledRig();
    const seat = (m: string) => ({ ...base.seats[0], id: `team.${m}@t`, pod: 'team', member: m, runtime: m === 'reviewer' ? ('codex' as const) : ('claude-code' as const) });
    const snap: RigSnapshot = {
      rig: { ...base, name: 't', pods: [{ id: 'team', label: 'team' }], seats: ['lead', 'builder', 'tester', 'reviewer'].map(seat), edges: [], launch_layers: [] },
      desired: 'stopped',
      seats: ['lead', 'builder', 'tester', 'reviewer'].map((m) => ({ ...seatView(m), id: `team.${m}@t`, rig: 't', pod: 'team', lifecycle: 'stopped' as const })),
      last_seq: 3,
    };
    act(() => FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM).at(-1)!.emit('snapshot', snap));
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Start team' }));
    expect(screen.getByRole('alertdialog')).toHaveAccessibleName('Start 3 Claude agents and 1 Codex agent?');
    expect(api.startTeam).not.toHaveBeenCalled();
    await user.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Start team' }));
    await waitFor(() => expect(api.startTeam).toHaveBeenCalledWith('t', 1, expect.any(String), false, false));
  });
});

describe('what a Start may cost', () => {
  const wide = () => teamDraft({
    agents: [draftAgent('lead', { kind: 'codex', name: 'Lead' }), ...Array.from({ length: 8 }, (_, i) => draftAgent(`w${i}`, { kind: 'codex', name: `Worker ${i}` }))],
    connections: Array.from({ length: 8 }, (_, i) => ({ id: `c${i}`, from: 'lead', to: `w${i}`, kind: 'hands-work-to' as const })),
  });

  it('says how many agents can work at once, and that nothing limits them', () => {
    const c = startConfirmation(wide(), '/home/me')!;
    expect(c.efficiency).toEqual([
      'Up to 8 agents can work at the same time: Lead hands work to 8 at once. Each uses your plan.',
      'Agents can start helpers without limit. Set Most agents and Levels of children in Settings → More settings.',
      'No run budget is set, so nothing stops this run before your plan runs out. Set one in Settings → Run budget.',
    ]);
  });

  it('states the run budget and the stop bound, and how many may work at once', () => {
    const d = { ...wide(), budget: { plan: [{ provider: 'claude', window: 'five_hour', hard: 30 }], tokens: { soft: 1_000_000, hard: 2_000_000 }, max_working: 4 } };
    expect(startConfirmation(d, '/home/me')!.efficiency).toEqual([
      'Up to 8 agents could work at the same time (Lead hands work to 8 at once); the run budget lets 4 work at once and the rest wait.',
      'Agents can start helpers without limit. Set Most agents and Levels of children in Settings → More settings.',
      'Run budget: Claude 5-hour: wrap up at 25.5%, stop at 30%; tokens: wrap up at 1.0M, stop at 2.0M.',
      'Sculptured wraps up first and stops early by about one model call for each agent working (up to 4 at once), so a run usually ends near each hard value. Usage is reported after each call, so it can pass a hard value by a little.',
    ]);
  });

  it('says nothing more for a small team with limits', () => {
    const d = teamDraft({
      agents: [draftAgent('a', { kind: 'claude', advanced: { model: null, permission: 'unrestricted', resume: 'resume-if-possible', token_limit: { tokens: 1000, counts: 'all', include_subagents: true } } }), draftAgent('b', { kind: 'claude' })],
      connections: [{ id: 'c', from: 'a', to: 'b', kind: 'hands-work-to' }],
      limits: { max_agents: 6, max_child_levels: 1 },
    });
    expect(startConfirmation(d, '/home/me')!.efficiency).toEqual([]);
  });
});
