import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { forgetViewports } from '../shell/viewport';
import { teamRecord } from '../test-fixtures';
import { saveTiming } from '../useTeamDraft';
import { card, fakeTeam, renderAt, resetApi } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'workflow'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

beforeEach(() => {
  resetApi();
  forgetViewports();
  localStorage.clear();
  saveTiming.delayMs = 5;
});

/** A team whose Builder uses the built-in Kindex component. */
function teamWithKindex() {
  const rec = teamRecord();
  rec.draft.agents = rec.draft.agents.map((a) => (a.name === 'Builder' ? { ...a, advanced: { ...a.advanced, integrations: ['kindex'] } } : a));
  fakeTeam(rec);
}

describe('components in the redesigned pages', () => {
  it('shows an agent’s components as chips on its Design card, in both board styles', async () => {
    teamWithKindex();
    renderAt('/team/t/structure');
    await screen.findByRole('region', { name: 'Design board' });
    expect(within(card('Builder')).getByLabelText(/^Uses /)).toBeVisible();
    expect(within(card('Reviewer')).queryByLabelText(/^Uses /)).toBeNull();
    await userEvent.setup().click(within(screen.getByRole('group', { name: 'Board style' })).getByRole('button', { name: 'Logos' }));
    expect(within(card('Builder')).getByLabelText(/^Uses /)).toBeVisible();
  });

  it('puts the team’s components under Settings › More settings', async () => {
    teamWithKindex();
    renderAt('/p/t/settings');
    const more = await screen.findByText('More settings');
    expect(more.closest('summary')).toHaveTextContent('components');
    await userEvent.setup().click(more);
    expect(document.querySelector('.set-more')).toHaveTextContent(/Kindex/);
  });
});
