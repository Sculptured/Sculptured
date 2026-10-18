import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import type { CompiledRig } from '../generated/CompiledRig';
import type { RigSnapshot } from '../generated/RigSnapshot';
import type { SeatView } from '../generated/SeatView';
import { resetSeenCache } from '../seen';
import { forgetViewports } from '../shell/viewport';
import { compiledRig, ev, FakeEventSource, seatView, teamDraft, teamRecord } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { ALL_TEAMS_STREAM } from '../useTeams';
import { saveTiming } from '../useTeamDraft';
import { card, fakeTeam, renderAt, resetApi, where } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'importTeamFile', 'importMadl', 'importRun', 'deployments', 'deployment', 'deploymentEvents'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

/** Team `t` as the server compiles it: Builder hands work to Reviewer, in the main group. */
function rigT(): CompiledRig {
  const base = compiledRig();
  const seat = (m: string) => ({ ...base.seats[0], id: `team.${m}@t`, pod: 'team', member: m, label: null, cwd: '.' });
  return {
    ...base,
    name: 't',
    pods: [{ id: 'team', label: 'team' }],
    seats: [seat('builder'), seat('reviewer')],
    edges: [{ kind: 'delegates_to', from: 'team.builder@t', to: 'team.reviewer@t', enforced: true }],
    launch_layers: [['team.builder@t'], ['team.reviewer@t']],
  };
}
const viewT = (m: string, over: Partial<SeatView> = {}): SeatView => ({ ...seatView(m), id: `team.${m}@t`, rig: 't', pod: 'team', ...over });
const snapT = (over: Partial<RigSnapshot> = {}): RigSnapshot => ({ rig: rigT(), desired: 'running', seats: [viewT('builder'), viewT('reviewer')], last_seq: 5, ...over });
const rigSummary = { name: 't', desired: 'running' as const, seat_count: 2, running_count: 2, attention_count: 0 };
/** The one live connection every team shares (the latest one open); snapshots name their team. */
const streamFor = (_rig: string) => FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM && !i.closed).at(-1)!;

/** Team `t` has been started: it is in the rig list and its summary says so. */
function started(rec = teamRecord({ started_revision: 1, live: true })) {
  const server = fakeTeam(rec);
  vi.mocked(api.listRigs).mockResolvedValue([rigSummary]);
  vi.mocked(api.listTeams).mockResolvedValue([{ id: 't', name: rec.draft.name, agent_count: 2, updated_at_ms: 1, started: rigSummary }]);
  return server;
}

beforeEach(() => {
  resetApi();
  FakeEventSource.reset();
  forgetViewports();
  resetSeenCache();
  localStorage.clear();
  saveTiming.delayMs = 5;
});
afterEach(() => {
  saveTiming.delayMs = 500;
  vi.useRealTimers();
});

describe('New team: Start from…', () => {
  it('an empty home shows the gallery; a template creates the team and opens its board (Practice is not offered)', async () => {
    fakeTeam(teamRecord({ draft: teamDraft({ id: 'builder-and-reviewer', name: 'Builder and Reviewer' }) }));
    vi.mocked(api.listTeams).mockResolvedValue([]);
    vi.mocked(api.createTeam).mockImplementation(async () => teamRecord({ draft: teamDraft({ id: 'builder-and-reviewer', name: 'Builder and Reviewer' }) }));
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    expect(within(gallery).getByRole('radio', { name: /Claude/ })).toBeChecked();
    expect(within(gallery).queryByRole('radio', { name: /Practice/ })).toBeNull();
    const tpl = await within(gallery).findByRole('button', { name: 'Builder → Reviewer' });
    expect(tpl).toHaveAccessibleDescription(/One agent builds, then hands the work to a second agent to review\. 2 agents · Claude/);
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
    await userEvent.setup().click(tpl);
    expect(api.createTeam).toHaveBeenCalledWith('builder-reviewer', false, null, expect.any(String));
    await waitFor(() => expect(where()).toBe('/p/builder-and-reviewer/board?mode=design'));
    expect(await screen.findByRole('button', { name: /^Builder — / })).toBeInTheDocument();
  });

  it('with Claude (the default) it says real CLIs are used, and passes the typed name', async () => {
    vi.mocked(api.createTeam).mockRejectedValue(new ApiError(400, 'unknown_template', 'no template "x"'));
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const user = userEvent.setup();
    expect(within(gallery).getByText(/use your own CLI logins and may cost money/)).toBeInTheDocument();
    expect(await within(gallery).findByRole('button', { name: 'Solo agent' })).toHaveAccessibleDescription(/1 agent · Claude/);
    await user.type(within(gallery).getByRole('textbox', { name: 'Name' }), 'Docs crew');
    await user.click(within(gallery).getByRole('button', { name: 'Solo agent' }));
    expect(api.createTeam).toHaveBeenCalledWith('solo', false, 'Docs crew', expect.any(String));
    expect(await within(gallery).findByRole('alert')).toHaveTextContent('Could not create the team: That template is not available.');
  });

  it('with Codex chosen, every agent of the new project runs on Codex', async () => {
    const rec = teamRecord({ draft: teamDraft({ id: 'pair', name: 'Pair' }) });
    fakeTeam(rec);
    vi.mocked(api.listTeams).mockResolvedValue([]);
    vi.mocked(api.createTeam).mockResolvedValue(rec);
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const user = userEvent.setup();
    await user.click(within(gallery).getByRole('radio', { name: /Codex/ }));
    expect(within(gallery).getByText(/Codex agents use your own CLI logins/)).toBeInTheDocument();
    await user.click(await within(gallery).findByRole('button', { name: 'Solo agent' }));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalled());
    const saved = vi.mocked(api.saveTeam).mock.calls[0][2];
    expect(saved.agents.every((a) => a.kind === 'codex')).toBe(true);
  });

  it('n opens the New project page from a Run board', async () => {
    started();
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    await userEvent.setup().keyboard('n');
    expect(await screen.findByRole('region', { name: 'New project' })).toBeInTheDocument();
    expect(where()).toBe('/new');
  });

  it('Import from file sits under Advanced and lists what was not imported', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const advanced = within(gallery).getByText(/^Advanced/).closest('details')!;
    expect(advanced).toHaveAttribute('data-advanced');
    expect(advanced).not.toHaveAttribute('open');
    const user = userEvent.setup();
    await user.click(within(advanced).getByText(/^Advanced/));
    vi.mocked(api.importTeam).mockResolvedValue({
      team: teamRecord({ draft: teamDraft({ id: 'imp', name: 'Imported' }) }),
      dropped: [{ path: 'pods[0].members[0].profile', severity: 'warning', message: 'profile "fast" is not kept' }],
    });
    await user.type(within(advanced).getByRole('textbox', { name: 'Team file contents' }), 'version: 1');
    await user.click(within(advanced).getByRole('button', { name: 'Import' }));
    const done = await within(advanced).findByRole('status');
    expect(done).toHaveTextContent('Imported Imported. 1 part was not imported:');
    expect(within(done).getByRole('list', { name: 'Not imported' })).toHaveTextContent('profile "fast" is not kept');
    await user.click(within(done).getByRole('button', { name: 'Open Imported' }));
    await waitFor(() => expect(where()).toBe('/p/imp/board?mode=design'));
  });

  it('imports a JSON team file, and lists every issue when it cannot', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const advanced = within(gallery).getByText(/^Advanced/).closest('details')!;
    const user = userEvent.setup();
    await user.click(within(advanced).getByText(/^Advanced/));
    const box = within(advanced).getByRole('textbox', { name: 'Team file contents' });
    vi.mocked(api.importTeamFile).mockRejectedValueOnce(
      new ApiError(422, 'team_invalid', '2 things to fix', undefined, [
        { element: { kind: 'connection', id: 'c1' }, severity: 'error', code: 'connection-dangling', message: 'An arrow points at an agent that is not there', detail: null, spec_path: null },
        { element: { kind: 'agent', id: 'a' }, severity: 'error', code: 'agent-id-invalid', message: 'Two agents share one id', detail: null, spec_path: null },
      ]),
    );
    await user.click(box);
    await user.paste('{"format":"sculptured.team/v1","team":{}}');
    await user.click(within(advanced).getByRole('button', { name: 'Import' }));
    const alert = await within(advanced).findByRole('alert');
    expect(alert).toHaveTextContent('An arrow points at an agent that is not there');
    expect(alert).toHaveTextContent('Two agents share one id');
    expect(api.importTeam).not.toHaveBeenCalled();
    vi.mocked(api.importTeamFile).mockResolvedValueOnce({ team: teamRecord({ draft: teamDraft({ id: 'imp', name: 'Imported' }) }), warnings: [] });
    await user.click(within(advanced).getByRole('button', { name: 'Import' }));
    await waitFor(() => expect(where()).toBe('/p/imp/board?mode=design'));
  });

  it('imports a MADL file (YAML or JSON) and lists what Sculptured could not keep', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const advanced = within(gallery).getByText(/^Advanced/).closest('details')!;
    const user = userEvent.setup();
    await user.click(within(advanced).getByText(/^Advanced/));
    vi.mocked(api.importMadl).mockResolvedValue({
      team: teamRecord({ draft: teamDraft({ id: 'imp', name: 'CAD review' }) }),
      dropped: [{ path: '/channels/0', severity: 'warning', message: 'Sculptured does not use channels yet' }],
    });
    const box = within(advanced).getByRole('textbox', { name: 'Team file contents' });
    const text = 'madl: draft\nsystem: { id: cad, name: CAD review }\n';
    await user.click(box);
    await user.paste(text);
    await user.click(within(advanced).getByRole('button', { name: 'Import' }));
    const done = await within(advanced).findByRole('status');
    expect(api.importMadl).toHaveBeenLastCalledWith(text, expect.any(String));
    expect(api.importTeam).not.toHaveBeenCalled();
    expect(done).toHaveTextContent('Imported CAD review. 1 part was not imported:');
    expect(within(done).getByRole('list', { name: 'Not imported' })).toHaveTextContent('Sculptured does not use channels yet');
    await user.clear(box);
    await user.click(box);
    await user.paste('{"madl":"draft","system":{"id":"cad","name":"CAD review"}}');
    await user.click(within(advanced).getByRole('button', { name: 'Import' }));
    await waitFor(() => expect(api.importMadl).toHaveBeenCalledTimes(2));
    expect(api.importTeamFile).not.toHaveBeenCalled();
  });

  it('imports a past run and opens its history', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const advanced = within(gallery).getByText(/^Advanced/).closest('details')!;
    const user = userEvent.setup();
    await user.click(within(advanced).getByText(/^Advanced/));
    vi.mocked(api.deployments).mockResolvedValue([]);
    vi.mocked(api.importRun).mockResolvedValue({ team: teamRecord({ draft: teamDraft({ id: 'past', name: 'Past' }) }), run: { start_seq: -1, started_ms: 1, initial_agents: 1, source: 'sculptured' }, warnings: [] });
    const file = new File(['{"format":"sculptured.session/v1"}\n'], 'r.sculptured-session.jsonl', { type: 'application/x-ndjson' });
    await user.upload(within(advanced).getByLabelText('Session or run files'), file);
    await waitFor(() => expect(api.importRun).toHaveBeenCalledWith('{"format":"sculptured.session/v1"}\n'));
    await waitFor(() => expect(where()).toBe('/p/past/results/history'));
  });

  it('a past run whose team was left out says why before opening it', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const advanced = within(gallery).getByText(/^Advanced/).closest('details')!;
    const user = userEvent.setup();
    await user.click(within(advanced).getByText(/^Advanced/));
    vi.mocked(api.deployments).mockResolvedValue([]);
    vi.mocked(api.importRun).mockResolvedValue({
      team: teamRecord({ draft: teamDraft({ id: 'past', name: 'Past (imported run)' }) }),
      run: { start_seq: -1, started_ms: 1, initial_agents: 1, source: 'sculptured' },
      warnings: [{ element: { kind: 'team', id: '' }, severity: 'warning', code: 'run-team-left-out', message: "The run's team could not be imported, so this team is empty: 1 thing to fix", detail: null, spec_path: null }],
    });
    await user.upload(within(advanced).getByLabelText('Session or run files'), new File(['x'], 'r.sculptured-session.jsonl'));
    const note = await within(advanced).findByRole('status');
    expect(note).toHaveTextContent("The run's team could not be imported");
    expect(where()).toBe('/');
    await user.click(within(note).getByRole('button', { name: 'Open Past (imported run)' }));
    await waitFor(() => expect(where()).toBe('/p/past/results/history'));
  });

  it('offers Describe it, which opens the generator', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const user = userEvent.setup();
    await user.click(within(gallery).getByRole('button', { name: /Describe it/ }));
    expect(await screen.findByRole('textbox', { name: 'Describe the team' })).toBeInTheDocument();
  });

  it('a team that never ran here opens its imported run in History', async () => {
    const run = { start_seq: -1, started_ms: 1, initial_agents: 1, source: 'sculptured' };
    vi.mocked(api.deployments).mockResolvedValue([run]);
    vi.mocked(api.deployment).mockResolvedValue({ run, agents: [], has_snapshot: true, stopped_ms: 2, summary_truncated: false });
    renderAt('/rig/past?view=history');
    expect(await screen.findByLabelText('Historical deployment')).toBeInTheDocument();
    expect(await screen.findByText(/Imported from a file/)).toBeVisible();
  });

  it('a file with errors shows them, under Advanced only', async () => {
    renderAt('/');
    const gallery = await screen.findByRole('region', { name: 'New project' });
    const advanced = within(gallery).getByText(/^Advanced/).closest('details')!;
    const user = userEvent.setup();
    await user.click(within(advanced).getByText(/^Advanced/));
    vi.mocked(api.importTeam).mockRejectedValue(
      new ApiError(422, 'spec_invalid', 'the spec has errors', [{ path: 'pods[0].members[0].runtime', severity: 'error', message: 'unknown runtime "gpt"' }]),
    );
    await user.type(within(advanced).getByRole('textbox', { name: 'Team file contents' }), 'x');
    await user.click(within(advanced).getByRole('button', { name: 'Import' }));
    const alert = await within(advanced).findByRole('alert');
    expect(within(alert).getByRole('list', { name: 'Problems in the file' })).toHaveTextContent('unknown runtime "gpt"');
    expect(alert.closest('[data-advanced]')).not.toBeNull();
  });
});

describe('Leaving the Structure board', () => {
  it('an edit made just before navigating away is saved, without waiting for autosave', async () => {
    const server = fakeTeam();
    saveTiming.delayMs = 10_000;
    renderAt('/team/t/structure');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: /^Builder — / }));
    await user.clear(screen.getByRole('textbox', { name: 'Name' }));
    await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Maker');
    expect(api.saveTeam).not.toHaveBeenCalled();
    await user.click(screen.getByRole('link', { name: /all projects/ }));
    await waitFor(() => expect(where()).toBe('/'));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalled());
    expect(server.rec.draft.agents.find((a) => a.id === 'builder')?.name).toBe('Maker');
  });
});

describe('A team that could not be loaded', () => {
  it('says why, and n opens New project', async () => {
    vi.mocked(api.getTeam).mockRejectedValue(new ApiError(404, 'not_found', 'no team'));
    renderAt('/team/t/structure');
    expect(await screen.findByText(/^Could not load this team:/)).toBeInTheDocument();
    await act(async () => {});
    await userEvent.setup().keyboard('n');
    expect(await screen.findByRole('region', { name: 'New project' })).toBeInTheDocument();
  });
});

describe('Start team → Live', () => {
  it('saves first, starts with the saved revision, and opens the Live board with draft names and the warnings', async () => {
    fakeTeam();
    const order: string[] = [];
    vi.mocked(api.saveTeam).mockImplementation(async (_i: string, revision: number, draft) => {
      order.push(`save ${revision}`);
      return teamRecord({ draft: { ...draft, revision: revision + 1 } });
    });
    vi.mocked(api.startTeam).mockImplementation(async (_id: string, revision: number) => {
      order.push(`start ${revision}`);
      return { rig: 't', warnings: [{ element: { kind: 'agent', id: 'reviewer' }, severity: 'warning', code: 'codex', message: 'Codex is not available yet', detail: null, spec_path: null }] };
    });
    saveTiming.delayMs = 10_000;
    renderAt('/team/t/structure');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: /^Builder — / }));
    await user.clear(await screen.findByRole('textbox', { name: 'Name' }));
    await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Maker');
    // Start team is in the project bar on every page, here the agent's Setup.
    await user.click(screen.getByRole('button', { name: 'Start team' }));
    await waitFor(() => expect(where()).toBe('/p/t/board?mode=run'));
    expect(order).toEqual(['save 1', 'start 2']);
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    expect(await screen.findByRole('button', { name: 'Maker — idle' })).toBeInTheDocument();
    const warn = screen.getByRole('status', { name: 'Start warnings' });
    expect(warn).toHaveTextContent('Started with a warning: Codex is not available yet');
    expect(within(screen.getByRole('group', { name: 'Board mode' })).getByRole('button', { name: /Design/ })).toBeEnabled();
  });

  it('the Live board uses the draft’s positions, and its list fallback the draft’s names', async () => {
    started(teamRecord({ draft: teamDraft({ agents: [{ ...teamDraft().agents[0], x: 520, y: 360 }, teamDraft().agents[1]] }) }));
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    await waitFor(() => expect((screen.getByRole('button', { name: 'Builder — idle' }).closest('.card-slot') as HTMLElement).style.left).toBe('520px'));
    // l opens the Agents list, the board's text fallback, with the design's names.
    await userEvent.setup().keyboard('l');
    expect(where()).toBe('/p/t/agents');
    expect(screen.getByRole('link', { name: /Reviewer/ })).toBeInTheDocument();
  });
});

describe('Stop team', () => {
  it('confirms, uses the team API, and lists agents that could not be stopped with the host’s reason', async () => {
    started();
    vi.mocked(api.stopTeam).mockResolvedValue({ failed: [{ seat: 'team.reviewer@t', reason: 'no seat process found on host local' }] });
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    expect(screen.getByRole('group', { name: 'Confirm stop team' })).toHaveTextContent('Stops every running agent in My team.');
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    expect(api.stopTeam).toHaveBeenCalledWith('t', expect.any(String));
    expect(api.stopRig).not.toHaveBeenCalled();
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('1 agent could not be stopped:');
    expect(within(alert).getByRole('list', { name: 'Agents not stopped' })).toHaveTextContent('Reviewer — no seat process found on host local');
    expect(within(alert).getByText('no seat process found on host local')).toHaveAttribute('data-raw-evidence');
  });

  it('while the team starts, Stop is disabled with the reason in words, and enabled once it has started', async () => {
    started();
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT({ seats: [viewT('builder'), viewT('reviewer', { lifecycle: 'launching' })] })));
    const stop = screen.getByRole('button', { name: 'Stop team…' });
    expect(stop).toBeDisabled();
    expect(stop).toHaveAccessibleDescription('Starting… you can stop the team once it has started');
    expect(screen.getByText('Starting… you can stop the team once it has started')).toBeVisible();
    act(() => streamFor('t').emit('snapshot', snapT()));
    expect(screen.getByRole('button', { name: 'Stop team…' })).toBeEnabled();
  });

  it('a stop refused because the team is still starting says so in the same plain words', async () => {
    started();
    vi.mocked(api.stopTeam).mockRejectedValue(new ApiError(409, 'team_starting', 'SERVER TEXT: still booting'));
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Starting… you can stop the team once it has started');
    expect(alert).not.toHaveTextContent('SERVER TEXT');
  });
});

describe('After Sculptured restarts', () => {
  const REASON = "This agent's process ended when Sculptured restarted. Start the team again to relaunch it.";
  const lost = () =>
    snapT({
      seats: [
        viewT('builder', { lifecycle: 'attention', lifecycle_reason: REASON, activity: 'unknown' }),
        viewT('reviewer', { lifecycle: 'attention', lifecycle_reason: REASON, activity: 'unknown' }),
      ],
    });

  it('the Live board says why nothing runs and offers Start team, which relaunches the saved version', async () => {
    started(teamRecord({ started_revision: 3, live: false, draft: teamDraft({ revision: 3 }) }));
    vi.mocked(api.startTeam).mockResolvedValue({ rig: 't', warnings: [] });
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', lost()));
    const note = screen.getByRole('status', { name: 'Not running' });
    expect(note).toHaveTextContent("None of this team's agents is running.");
    expect(within(note).getByText(REASON)).toHaveAttribute('data-raw-evidence');
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
    await userEvent.setup().click(screen.getByRole('button', { name: 'Start team' }));
    await waitFor(() => expect(api.startTeam).toHaveBeenCalledWith('t', 3, expect.any(String), false, false));
  });

  it('a start from Live that the server refuses says why in words and points to the Structure board', async () => {
    started(teamRecord({ started_revision: 1, live: false }));
    const issue = { element: { kind: 'agent' as const, id: 'builder' }, severity: 'error' as const, code: 'folder-outside-home', message: "Builder's folder is outside your home folder", detail: null, spec_path: null };
    vi.mocked(api.startTeam).mockRejectedValue(new ApiError(422, 'team_invalid', 'SERVER TEXT', undefined, [issue]));
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', lost()));
    await userEvent.setup().click(screen.getByRole('button', { name: 'Start team' }));
    const alert = await screen.findByRole('alertdialog', { name: 'The team could not start' });
    expect(alert).toHaveTextContent('1 thing to fix');
    expect(alert).toHaveTextContent("Builder's folder is outside your home folder");
    expect(within(alert).getByRole('link', { name: 'Design board' })).toHaveAttribute('href', '/p/t/board?mode=design');
    expect(alert).not.toHaveTextContent('SERVER TEXT');
  });

  it('a plan hold pops up and Start anyway starts past it', async () => {
    started(teamRecord({ started_revision: 1, live: false }));
    vi.mocked(api.startTeam).mockRejectedValueOnce(new ApiError(409, 'plan_limit', 'Codex plan is at 100% of its weekly limit. New work is held at 90%.')).mockResolvedValueOnce({ rig: 't', warnings: [] });
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', lost()));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Start team' }));
    const held = await screen.findByRole('alertdialog', { name: 'Your plan is near its limit' });
    expect(held).toHaveTextContent('Codex plan is at 100%');
    await user.click(within(held).getByRole('button', { name: 'Start anyway' }));
    await waitFor(() => expect(api.startTeam).toHaveBeenLastCalledWith('t', expect.any(Number), undefined, true, false));
  });

  it('the Structure board is not locked and Start team is enabled', async () => {
    started(teamRecord({ started_revision: 1, live: false }));
    renderAt('/team/t/structure');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', lost()));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Start team' })).toBeEnabled());
    expect(screen.queryByText(/^Running: you can rename agents/)).not.toBeInTheDocument();
  });

  it('a running team offers no Start on Live', async () => {
    started();
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    expect(screen.queryByRole('button', { name: 'Start team' })).not.toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Not running' })).not.toBeInTheDocument();
  });
});

describe('A running team on the Structure board', () => {
  it('locks structure with the reason in words, and keeps names and jobs editable', async () => {
    started();
    renderAt('/team/t/structure');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    expect(await screen.findByText(/Running: you can rename agents, edit jobs and move cards/)).toBeInTheDocument();
    const add = screen.getByRole('button', { name: 'Add a Claude agent' });
    expect(add).toBeDisabled();
    expect(add).toHaveAccessibleDescription('Stop the team to add components or change arrows.');
    const user = userEvent.setup();
    await user.click(card('Builder'));
    await user.click(await screen.findByRole('link', { name: 'Setup' }));
    expect(await screen.findByRole('button', { name: 'Delete agent…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Connect to…' })).toHaveAccessibleDescription('Stop the team to change this.');
    // One primary action: while it runs, Stop team…
    expect(screen.queryByRole('button', { name: 'Start team' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Stop team…' })).toBeEnabled();
    const name = screen.getByRole('textbox', { name: 'Name' });
    expect(name).toBeEnabled();
    await user.type(name, '!');
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalled());
    // No port to drag a new connection from.
    await user.click(screen.getByRole('link', { name: 'Board' }));
    expect((await screen.findByRole('button', { name: /^Builder! — / })).parentElement!.querySelector('.port.out')).toBeNull();
  });
});

describe('Teams on the Projects page and in the switcher', () => {
  it('follows a team that just started even before the next rig list, on its Design board too', async () => {
    started();
    vi.mocked(api.listRigs).mockResolvedValue([]);
    renderAt('/team/t/structure');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT()));
    expect(await screen.findByText(/Running: you can rename agents/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop team…' })).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole('link', { name: 'Agents' }));
    expect(await screen.findByRole('link', { name: /Builder/ })).toBeInTheDocument();
  });

  it('lists a team that has not started, and its Agents open each agent on Setup', async () => {
    fakeTeam();
    renderAt('/');
    const row = (await screen.findByRole('link', { name: 'My team' })).closest('tr')!;
    expect(row).toHaveTextContent('Draft · not started');
    const user = userEvent.setup();
    await user.click(within(row).getByRole('link', { name: '2 agents in My team' }));
    expect(where()).toBe('/p/t/agents');
    const table = await screen.findByRole('table');
    expect(within(table).getAllByText('not started').length).toBeGreaterThan(0);
    await user.click(within(table).getByRole('link', { name: /Reviewer/ }));
    expect(await screen.findByRole('region', { name: 'Setup' })).toBeInTheDocument();
    expect(where()).toBe('/p/t/agents/team.reviewer/setup');
  });

  it('does not toast a turn that ends on the board you are watching', async () => {
    started();
    renderAt('/rig/t');
    await waitFor(() => expect(streamFor('t')).toBeDefined());
    act(() => streamFor('t').emit('snapshot', snapT({ seats: [viewT('builder', { activity: 'working', current_turn: 'x' }), viewT('reviewer')] })));
    act(() =>
      streamFor('t').emit('event', {
        event: { ...ev(9, { type: 'turn-ended', turn_id: 'x', status: 'completed' }), subject: { kind: 'seat', id: 'team.builder@t' } },
        seat: viewT('builder'),
      }),
    );
    expect(await screen.findByRole('button', { name: 'Builder — done (reported)' })).toBeInTheDocument();
    expect(within(screen.getByRole('region', { name: 'Notifications' })).queryAllByRole('listitem')).toHaveLength(0);
  });
});

describe('Narrow screens', () => {
  const mq = (matches: boolean) =>
    vi.fn().mockImplementation((q: string) => ({ matches, media: q, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  afterEach(() => {
    // @ts-expect-error jsdom has no matchMedia; restore that.
    delete window.matchMedia;
  });

  it('Design opens on the board, and an agent is set up on its own one-column page', async () => {
    window.matchMedia = mq(true);
    fakeTeam();
    renderAt('/team/t/structure');
    expect(await screen.findByTestId('structure-viewport')).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(card('Builder'));
    const setup = await screen.findByRole('region', { name: 'Setup' });
    expect(within(setup).getByRole('textbox', { name: 'Job' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Start team' })).toBeEnabled();
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
  });
});
