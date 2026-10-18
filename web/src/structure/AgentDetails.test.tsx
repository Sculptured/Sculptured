import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { TeamDraft } from '../generated/TeamDraft';
import { forgetViewports } from '../shell/viewport';
import { teamRecord } from '../test-fixtures';
import { saveTiming } from '../useTeamDraft';
import { card, fakeTeam, renderAt, resetApi, where } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const saved = (): TeamDraft => vi.mocked(api.saveTeam).mock.calls.at(-1)![2];
const kindOf = (id: string) => saved().agents.find((a) => a.id === id)!.kind;

async function board() {
  fakeTeam(teamRecord());
  renderAt('/team/t/structure');
  await screen.findByRole('region', { name: 'Design board' });
  return userEvent.setup();
}

/** The agent's Setup tab, reached by clicking its card. */
async function setupOf(name: string) {
  const user = await board();
  await user.click(card(name));
  const setup = await screen.findByRole('region', { name: 'Setup' });
  return { user, setup };
}

beforeEach(() => {
  resetApi();
  forgetViewports();
  localStorage.clear();
  saveTiming.delayMs = 5;
});

describe('Agent page: Setup', () => {
  it('a card click opens the agent page on Setup, one roomy column: Basics, Job, Folder, Connections, then Advanced folded', async () => {
    const { user, setup } = await setupOf('Builder');
    expect(where()).toBe('/p/t/agents/team.builder');
    expect(screen.getByRole('heading', { level: 1, name: 'Builder' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Setup' })).toHaveAttribute('aria-current', 'page');
    for (const name of ['Basics', 'Job', 'Folder', 'Connections']) expect(within(setup).getByRole('region', { name })).toBeInTheDocument();
    const basics = within(setup).getByRole('region', { name: 'Basics' });
    expect(within(basics).getByRole('textbox', { name: 'Name' })).toHaveValue('Builder');
    // Runs on and its model sit on one row.
    expect(within(basics).getByRole('radiogroup', { name: 'Builder runs on' }).closest('.runs-on-row')).not.toBeNull();
    expect(within(setup).getByRole('textbox', { name: 'Job' })).toBeVisible();
    // No explanation paragraphs: the Job's note is a "?" hint and the field's description.
    for (const name of ['Basics', 'Job', 'Folder', 'Connections']) expect(within(setup).getByRole('region', { name }).querySelector('.fld-hint')).toBeNull();
    const advanced = setup.querySelector<HTMLDetailsElement>('.setup-advanced > details')!;
    expect(advanced).not.toHaveAttribute('open');
    expect(advanced.querySelector('summary')).toHaveTextContent('Advanced · group, limits, check');
    expect(advanced).toContainElement(within(setup).getByRole('textbox', { name: 'Check' }));
    expect(advanced).toContainElement(within(setup).getByText('Move to group…').closest('button'));
    expect(advanced.querySelector('[data-advanced]')).not.toBeNull();
    await user.click(advanced.querySelector('summary')!);
    expect(advanced).toHaveAttribute('open');
    expect(localStorage.getItem('sculptured.agent.more-settings.v1')).toBe('true');
    await user.type(within(setup).getByRole('textbox', { name: 'Check' }), 'npm test');
    expect(advanced.querySelector('summary')).toHaveTextContent('Advanced · check set');
    await waitFor(() => expect(saved().agents.find((a) => a.id === 'builder')!.advanced.check).toBe('npm test'));
  });

  it('switches the agent to Codex and back to Claude with a model; the edits autosave and show on the board', async () => {
    const { user, setup } = await setupOf('Builder');
    await user.click(within(setup).getByRole('radio', { name: /Codex/ }));
    await waitFor(() => expect(kindOf('builder')).toBe('codex'));
    expect(within(setup).getByRole('combobox', { name: 'Codex model for Builder' })).toBeInTheDocument();
    await user.click(within(setup).getByRole('radio', { name: /Claude/ }));
    await user.selectOptions(within(setup).getByRole('combobox', { name: 'Claude model for Builder' }), 'opus');
    await waitFor(() => expect(saved().agents.find((a) => a.id === 'builder')!.advanced.model).toBe('opus'));
    // Undo still works across pages: the design and its history belong to the project.
    await user.click(screen.getByRole('button', { name: /^Board/ }));
    expect(await screen.findByRole('region', { name: 'Design board' })).toBeInTheDocument();
    expect(card('Builder')).toHaveTextContent('Opus');
  });

  it('a problem shows once as a short line at the top, and beside the field it is about', async () => {
    const issue = { element: { kind: 'agent' as const, id: 'builder' }, severity: 'error' as const, code: 'folder-missing', message: 'Builder needs a folder to work in', detail: null, spec_path: null };
    fakeTeam(teamRecord({ issues: [issue] }));
    renderAt('/p/t/agents/team.builder/setup');
    const setup = await screen.findByRole('region', { name: 'Setup' });
    const top = within(setup).getByRole('status', { name: 'Problems' });
    expect(top).toHaveTextContent('1 to fix:');
    expect(within(top).getByRole('button', { name: 'Folder' })).toBeInTheDocument();
    expect(within(within(setup).getByRole('region', { name: 'Folder' })).getByText('Builder needs a folder to work in')).toBeInTheDocument();
  });

  it('Esc goes back to the board with its view and the selected card kept', async () => {
    const user = await board();
    const world = screen.getByTestId('structure-world');
    await user.click(screen.getByRole('button', { name: 'Zoom in' }));
    const zoomed = world.style.transform;
    await user.click(card('Reviewer'));
    await screen.findByRole('region', { name: 'Setup' });
    await user.keyboard('{Escape}');
    await screen.findByRole('region', { name: 'Design board' });
    // The board it was opened from, in the same mode.
    expect(where()).toBe('/p/t/board?mode=design');
    expect(screen.getByTestId('structure-world').style.transform).toBe(zoomed);
    await waitFor(() => expect(card('Reviewer')).toHaveFocus());
  });

  it('right-click on a card still offers Runs on Codex', async () => {
    const user = await board();
    fireEvent.contextMenu(card('Reviewer'), { clientX: 200, clientY: 200 });
    const menu = screen.getByRole('menu', { name: 'Reviewer actions' });
    await user.click(within(menu).getByRole('menuitemradio', { name: /Runs on Codex/ }));
    await waitFor(() => expect(kindOf('reviewer')).toBe('codex'));
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});

describe('Settings: default models', () => {
  it('switches every agent at once and sets the team Claude model, shown on the cards', async () => {
    fakeTeam(teamRecord());
    renderAt('/p/t/settings');
    const user = userEvent.setup();
    const models = await screen.findByRole('region', { name: 'Default models' });
    await user.click(within(models).getByRole('radio', { name: /Claude/ }));
    await waitFor(() => expect(saved().agents.every((a) => a.kind === 'claude')).toBe(true));
    await user.selectOptions(within(models).getByRole('combobox', { name: 'Team: Claude model' }), 'sonnet');
    await waitFor(() => expect(saved().models?.claude).toBe('sonnet'));
    await user.click(screen.getByRole('link', { name: 'Board' }));
    await screen.findByRole('region', { name: 'Design board' });
    expect(card('Reviewer')).toHaveTextContent('Sonnet');
  });
});
