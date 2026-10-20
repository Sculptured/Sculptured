import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import { forgetViewports } from '../shell/viewport';
import { teamRecord } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { saveTiming } from '../useTeamDraft';
import { fakeTeam, renderAt, resetApi } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'teamSpec', 'teamMadl'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const yaml = 'version: "0.2"\nname: t\npods: []\n# sculptured: {"name":"My team"}\n';

/** Settings → Export, with its Advanced (the spec) opened. Returns the Export section. */
async function openAdvanced() {
  renderAt('/p/t/settings');
  const exp = await screen.findByRole('region', { name: 'Export' });
  const details = within(exp).getByText('Advanced', { selector: 'summary' }).closest('details')!;
  expect(details).not.toHaveAttribute('open');
  expect(details).toHaveAttribute('data-advanced');
  const user = userEvent.setup();
  await user.click(within(details).getByText('Advanced', { selector: 'summary' }));
  expect(details).toHaveAttribute('open');
  return { advanced: exp, user };
}

describe('team Settings: export, the spec and more settings', () => {
  const clicked: HTMLAnchorElement[] = [];
  beforeEach(() => {
    resetApi();
    forgetViewports();
    localStorage.clear();
    saveTiming.delayMs = 5;
    clicked.length = 0;
    fakeTeam(teamRecord());
    vi.mocked(api.teamSpec).mockResolvedValue({ yaml, valid: true, issues: [] });
    URL.createObjectURL = vi.fn(() => 'blob:spec');
    URL.revokeObjectURL = vi.fn();
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push(this);
    });
  });
  afterEach(() => {
    saveTiming.delayMs = 500;
    vi.restoreAllMocks();
  });

  it('is closed by default and shows the spec read-only, with Copy', async () => {
    const { advanced, user } = await openAdvanced();
    await user.click(within(advanced).getByRole('button', { name: 'View as spec' }));
    const spec = await within(advanced).findByRole('region', { name: 'Team spec' });
    expect(spec).toHaveTextContent('This is the OpenRig format Sculptured uses internally');
    expect(spec.querySelector('pre')).toHaveTextContent('name: t');
    expect(spec.querySelector('textarea, input')).toBeNull();
    await user.click(within(spec).getByRole('button', { name: 'Copy' }));
    expect(await navigator.clipboard.readText()).toBe(yaml);
    expect(within(spec).getByText('Copied')).toBeInTheDocument();
    // Spec words are allowed inside Advanced only.
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
  });

  it('exports {teamId}.rig.yaml through a Blob download, after saving pending edits', async () => {
    saveTiming.delayMs = 10_000;
    const { advanced, user } = await openAdvanced();
    const name = screen.getByRole('textbox', { name: 'Team name' });
    await user.type(name, '!');
    const order: string[] = [];
    vi.mocked(api.saveTeam).mockImplementation(async (_id, revision, draft) => {
      order.push('save');
      return teamRecord({ draft: { ...draft, revision: revision + 1 } });
    });
    vi.mocked(api.teamSpec).mockImplementation(async () => {
      order.push('spec');
      return { yaml, valid: true, issues: [] };
    });
    await user.click(within(advanced).getByRole('button', { name: 'Export' }));
    await waitFor(() => expect(clicked).toHaveLength(1));
    expect(order).toEqual(['save', 'spec']);
    expect(clicked[0].download).toBe('t.rig.yaml');
    expect(clicked[0].href).toBe('blob:spec');
    expect(URL.createObjectURL).toHaveBeenCalledWith(expect.any(Blob));
    expect(within(advanced).getByRole('status')).toHaveTextContent('Exported t.rig.yaml');
  });

  it('exports the team file as MADL, without folders unless asked', async () => {
    vi.mocked(api.teamMadl).mockResolvedValue({ yaml: 'madl: draft\n' });
    const { advanced, user } = await openAdvanced();
    await user.click(within(advanced).getByRole('button', { name: 'Export team file' }));
    await waitFor(() => expect(clicked).toHaveLength(1));
    expect(api.teamMadl).toHaveBeenLastCalledWith('t', false);
    expect(clicked[0].download).toBe('t.madl.yaml');
    expect(within(advanced).getByRole('status')).toHaveTextContent('Exported t.madl.yaml');
    const blob = vi.mocked(URL.createObjectURL).mock.calls.at(-1)![0] as Blob;
    expect(await blob.text()).toBe('#!/usr/bin/env -S sculptured run\nmadl: draft\n');
    await user.click(within(advanced).getByRole('checkbox', { name: /Include folders/ }));
    await user.click(within(advanced).getByRole('button', { name: 'Export team file' }));
    await waitFor(() => expect(clicked).toHaveLength(2));
    expect(api.teamMadl).toHaveBeenLastCalledWith('t', true);
    expect(within(advanced).queryByRole('button', { name: 'Export as MADL' })).toBeNull();
  });

  it('sets the team\'s own limits on spawned agents; blank means no limit', async () => {
    const state = fakeTeam(teamRecord());
    renderAt('/p/t/settings');
    const user = userEvent.setup();
    const advanced = (await screen.findByText('More settings')).closest('details')!;
    await user.click(within(advanced).getByText('More settings'));
    const most = within(advanced).getByRole('spinbutton', { name: 'Most agents, children included' });
    expect(most).toHaveValue(null);
    expect(within(advanced).getByText(/Sculptured sets no limits of its own/)).toBeInTheDocument();
    await user.type(most, '20');
    await user.type(within(advanced).getByRole('spinbutton', { name: 'Most levels of children' }), '3');
    await waitFor(() => expect(state.rec.draft.limits).toEqual({ max_agents: 20, max_child_levels: 3 }));
    await user.clear(most);
    await waitFor(() => expect(state.rec.draft.limits).toEqual({ max_child_levels: 3 }));
    await user.clear(within(advanced).getByRole('spinbutton', { name: 'Most levels of children' }));
    await waitFor(() => expect(state.rec.draft.limits).toBeUndefined());
  });

  it('a generated team says so, with its description', async () => {
    fakeTeam(teamRecord({ origin: { kind: 'generated', cli: 'claude', model: null, at_ms: Date.UTC(2026, 9, 12), prompt: 'Rebuild our docs site' } }));
    renderAt('/p/t/settings');
    const team = await screen.findByRole('region', { name: 'Team' });
    expect(await within(team).findByText(/Generated by Claude Code from a description/)).toBeInTheDocument();
    expect(within(team).getByText(/not run yet/)).toBeInTheDocument();
    expect(within(team).getByText('Rebuild our docs site')).toBeInTheDocument();
  });

  it('says what the team needs before it can run, from the spec check', async () => {
    vi.mocked(api.teamSpec).mockResolvedValue({
      yaml,
      valid: false,
      issues: [
        { element: { kind: 'agent', id: 'builder' }, severity: 'error', code: 'folder-missing', message: 'Builder needs a folder to work in', detail: 'cwd is required', spec_path: 'pods[0].members[0].cwd' },
        { element: { kind: 'agent', id: 'reviewer' }, severity: 'error', code: 'folder-missing', message: 'Reviewer needs a folder to work in', detail: null, spec_path: null },
      ],
    });
    const { advanced, user } = await openAdvanced();
    await user.click(within(advanced).getByRole('button', { name: 'View as spec' }));
    const spec = await within(advanced).findByRole('region', { name: 'Team spec' });
    expect(spec).toHaveTextContent('This team needs a folder for Builder and Reviewer before it can run.');
    expect(spec).toHaveTextContent('pods[0].members[0].cwd');
  });

  it('a spec that cannot be read says why, inside Advanced', async () => {
    vi.mocked(api.teamSpec).mockRejectedValue(new ApiError(500, 'internal', 'the store is locked'));
    const { advanced, user } = await openAdvanced();
    await user.click(within(advanced).getByRole('button', { name: 'View as spec' }));
    expect(await within(advanced).findByRole('alert')).toHaveTextContent('Could not read the spec: Sculptured hit an internal problem.');
    expect(clicked).toHaveLength(0);
  });
});
