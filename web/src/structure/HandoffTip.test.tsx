import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { forgetViewports } from '../shell/viewport';
import { teamDraft, teamRecord } from '../test-fixtures';
import { fakeTeam, renderAt, resetApi } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

beforeEach(() => {
  resetApi();
  forgetViewports();
  localStorage.clear();
});

describe('Structure board: what a connection does', () => {
  it('explains a "hands work to" connection as a handoff, and leaves the others alone', async () => {
    const draft = teamDraft({
      connections: [
        { id: 'c1', from: 'builder', to: 'reviewer', kind: 'hands-work-to' },
        { id: 'c2', from: 'reviewer', to: 'builder', kind: 'watches' },
      ],
    });
    fakeTeam(teamRecord({ draft }));
    renderAt('/team/t/structure');
    await screen.findByRole('region', { name: 'Design board' });
    const hands = document.querySelector('[data-connection="c1"]')!;
    expect(hands).toHaveAttribute('data-tip', "Reviewer starts when Builder finishes; Builder's result is passed along");
    const watches = document.querySelector('[data-connection="c2"]')!;
    expect(watches).not.toHaveAttribute('data-tip');
  });

  it("says in the Job field, as its ? hint, when an agent's Job is sent", async () => {
    fakeTeam(teamRecord());
    renderAt('/team/t/structure');
    await screen.findByRole('region', { name: 'Design board' });
    const user = userEvent.setup();
    // A card opens its agent's page; before a first start that is Setup.
    await user.click(screen.getByRole('button', { name: /^Reviewer — / }));
    expect(await screen.findByRole('textbox', { name: 'Job' })).toHaveAccessibleDescription(
      'Sent with its first handoff, as it reads then: you can still change it while this agent waits.',
    );
    await user.keyboard('{Escape}');
    await user.click(await screen.findByRole('button', { name: /^Builder — / }));
    expect(await screen.findByRole('textbox', { name: 'Job' })).toHaveAccessibleDescription(
      'Sent as its first message when the team starts. With no Job, your first message to it is its work.',
    );
  });
});
