import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError, type DesignProposal } from '../../api';
import type { TeamDraft } from '../../generated/TeamDraft';
import { forgetViewports } from '../../shell/viewport';
import { draftAgent, teamDraft, teamRecord } from '../../test-fixtures';
import { saveTiming } from '../../useTeamDraft';
import { card, fakeTeam, renderAt, resetApi } from '../harness';

vi.mock('../../api', async (orig) => {
  const m = await orig<typeof import('../../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'designAssistant'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

/** The saved design plus a QA agent after the reviewer, and a sharper builder job. */
function proposal(summary = 'Added QA after the reviewer and made the builder job specific.'): DesignProposal {
  const d = teamDraft();
  const draft: TeamDraft = {
    ...d,
    agents: [{ ...d.agents[0], job: 'Build the feature with tests.' }, d.agents[1], draftAgent('qa', { name: 'QA', x: 560, y: 40, job: 'Run the tests.' })],
    connections: [...d.connections, { id: 'reviewer-handoff-qa', from: 'reviewer', to: 'qa', kind: 'hands-work-to' }],
  };
  return { summary, madl: 'madl: draft\n# proposal\n', draft, dropped: [], issues: [] };
}

async function askFor(text: string) {
  const panel = screen.getByRole('region', { name: 'Design assistant' });
  await userEvent.type(within(panel).getByRole('textbox'), text);
  await userEvent.click(within(panel).getByRole('button', { name: /^(Ask|Send)$/ }));
  return panel;
}

async function openAssistant() {
  fakeTeam(teamRecord());
  renderAt('/team/t/structure');
  await screen.findByRole('region', { name: 'Design board' });
  await userEvent.click(screen.getByRole('button', { name: 'Design assistant' }));
}

beforeEach(() => {
  resetApi();
  forgetViewports();
  localStorage.clear();
  localStorage.setItem('sculptured.panels.v2.right', 'true');
  saveTiming.delayMs = 5;
});
afterEach(() => {
  saveTiming.delayMs = 500;
});

describe('Design assistant', () => {
  it('shows the proposal on the board without saving it, and approving makes it the design', async () => {
    vi.mocked(api.designAssistant).mockResolvedValue(proposal());
    await openAssistant();
    const panel = await askFor('Add a QA agent after the reviewer');
    expect(api.designAssistant).toHaveBeenLastCalledWith('t', { options: { cli: 'claude', model: null }, messages: [{ role: 'person', text: 'Add a QA agent after the reviewer' }], proposal: null });
    const bar = await screen.findByRole('region', { name: 'Proposal shown on the board' });
    expect(bar).toHaveTextContent('3 changes');
    expect(card('QA')).toHaveTextContent('New');
    expect(card('Builder')).toHaveTextContent('Changed');
    const changes = within(panel).getByRole('list', { name: 'Proposed changes' });
    expect(changes).toHaveTextContent('Changed Builder — job');
    expect(changes).toHaveTextContent('New QA');
    expect(changes).toHaveTextContent('New Reviewer → QA');
    expect(screen.getByRole('button', { name: 'Start team' })).toBeDisabled();
    expect(screen.getAllByRole('button', { name: /^Add a .* agent$/ })[0]).toBeDisabled();
    expect(screen.getByText('Approve or reject the proposal to change the board.')).toBeInTheDocument();
    expect(api.saveTeam).not.toHaveBeenCalled();

    // A proposed agent is not saved yet, so it has no page: the board shows it and selects it.
    await userEvent.click(within(changes).getByRole('button', { name: 'QA' }));
    await waitFor(() => expect(card('QA')).toHaveFocus());

    await userEvent.click(within(bar).getByRole('button', { name: 'Approve' }));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalled());
    const saved = vi.mocked(api.saveTeam).mock.calls.at(-1)![2];
    expect(saved.agents.map((a) => a.id)).toEqual(['builder', 'reviewer', 'qa']);
    expect(saved.agents[0].job).toBe('Build the feature with tests.');
    expect(screen.queryByRole('region', { name: 'Proposal shown on the board' })).not.toBeInTheDocument();
    expect(within(panel).getByRole('status')).toHaveTextContent('Approved: the proposal is now the design.');
    expect(screen.getAllByRole('button', { name: /^Add a .* agent$/ })[0]).toBeEnabled();
  });

  it('says plainly that the CLI is designing, for as long as it takes', async () => {
    let answer: (p: DesignProposal) => void = () => {};
    vi.mocked(api.designAssistant).mockReturnValue(new Promise((r) => { answer = r; }));
    await openAssistant();
    const panel = await askFor('Connect every component like electrical and mechanical systems');
    const working = await screen.findByRole('status', { name: 'Design assistant is working' });
    expect(working).toHaveTextContent('Claude Code is designing your change');
    expect(working).toHaveTextContent(/0:0\d/);
    expect(within(panel).getByText('Connect every component like electrical and mechanical systems')).toBeInTheDocument();
    expect(within(panel).getByText(/can take several minutes/)).toBeInTheDocument();
    expect(within(panel).getByRole('button', { name: 'Ask' })).toBeDisabled();
    answer(proposal());
    expect(await screen.findByRole('region', { name: 'Proposal shown on the board' })).toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Design assistant is working' })).not.toBeInTheDocument();
  });

  it('sends on Enter, and gives the message back when the request fails', async () => {
    vi.mocked(api.designAssistant).mockRejectedValueOnce(new ApiError(504, 'workspace_timeout', 'Your workspace took too long to answer. Try again.'));
    await openAssistant();
    const panel = screen.getByRole('region', { name: 'Design assistant' });
    const box = within(panel).getByRole('textbox');
    await userEvent.type(box, 'Add a tester{Enter}');
    expect(await within(panel).findByRole('alert')).toHaveTextContent('Claude Code took longer than Sculptured waits');
    expect(box).toHaveValue('Add a tester');
    expect(within(panel).queryByRole('list', { name: 'Conversation' })).not.toBeInTheDocument();
  });

  it('rejecting leaves the design as it was', async () => {
    vi.mocked(api.designAssistant).mockResolvedValue(proposal());
    await openAssistant();
    const panel = await askFor('Add a QA agent');
    await screen.findByRole('region', { name: 'Proposal shown on the board' });
    await userEvent.click(within(panel).getByRole('button', { name: 'Reject' }));
    expect(screen.queryByRole('button', { name: /^QA — / })).not.toBeInTheDocument();
    expect(card('Builder')).not.toHaveTextContent('Changed');
    expect(within(panel).getByRole('status')).toHaveTextContent('Rejected: the design is as it was.');
    await new Promise((r) => setTimeout(r, 30));
    expect(api.saveTeam).not.toHaveBeenCalled();
  });

  it('keeps iterating from the proposal with the whole conversation', async () => {
    vi.mocked(api.designAssistant).mockResolvedValueOnce(proposal('Added QA.')).mockResolvedValueOnce({ ...proposal('Renamed QA to Tester.'), madl: 'madl: draft\n# second\n' });
    await openAssistant();
    const panel = await askFor('Add a QA agent');
    await within(panel).findByText('Added QA.');
    await askFor('Call it Tester');
    await within(panel).findByText('Renamed QA to Tester.');
    expect(api.designAssistant).toHaveBeenLastCalledWith('t', {
      options: { cli: 'claude', model: null },
      messages: [{ role: 'person', text: 'Add a QA agent' }, { role: 'assistant', text: 'Added QA.' }, { role: 'person', text: 'Call it Tester' }],
      proposal: 'madl: draft\n# proposal\n',
    });
  });
});
