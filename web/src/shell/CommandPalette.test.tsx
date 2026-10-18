import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { STATES } from '../herdr';
import { CommandPalette } from './CommandPalette';
import { readRecent, type Command } from './commands';
import type { AgentRow, TeamRow } from './model';

const agent = (id: string, name: string): AgentRow =>
  ({ id, rig: 'r', name, group: 'Main', folder: `/work/${id}`, seat: undefined, state: { ...STATES.idle, key: 'idle' }, vendor: { cls: 'v-claude', name: 'Claude' }, summary: 'idle', lastMs: null, activity: undefined }) as unknown as AgentRow;

const teams: TeamRow[] = [
  { rig: 'r', title: 'Web shop', agents: [agent('a', 'Builder'), agent('b', 'Reviewer')], counts: {} as TeamRow['counts'], connected: true, loaded: true, names: {}, href: '/rig/r' },
];

function commands(run = vi.fn()): Command[] {
  return [
    { id: 'team.start', section: 'Team', title: 'Start team…', rank: 1, disabled: 'Fix 2 things first.', run },
    { id: 'board.add-group', section: 'Design board', title: 'Add a group', keys: 'g g', rank: 2, run },
    { id: 'run.list', section: 'Run view', title: 'Show the list', keys: 'l', rank: 3, run },
    { id: 'app.library', section: 'Go to', title: 'Library', run },
  ];
}

function open(cmds = commands(), over: Partial<Parameters<typeof CommandPalette>[0]> = {}) {
  const onClose = vi.fn();
  const onJump = vi.fn();
  render(<CommandPalette teams={teams} commands={cmds} project="r" now={0} onClose={onClose} onJump={onJump} {...over} />);
  const dialog = screen.getByRole('dialog', { name: over.scope === 'agents' ? 'Go to agent' : 'Command palette' });
  return { dialog, onClose, onJump, names: () => within(dialog).queryAllByRole('option').map((o) => o.getAttribute('aria-label')) };
}

beforeEach(() => localStorage.clear());

describe('Command palette', () => {
  it('before you type, lists a short set: actions that can run, then the open project agents', () => {
    const { dialog, names } = open();
    // Start cannot run now, so it is not suggested; Library has no rank, so it waits for a search.
    expect(names()).toEqual(['Add a group', 'Show the list', 'Builder — idle', 'Reviewer — idle']);
    expect(within(dialog).getByRole('group', { name: 'Agents · Web shop' })).toBeInTheDocument();
    // Each command shows its key.
    expect(within(dialog).getByRole('option', { name: 'Show the list' })).toHaveTextContent('l');
  });

  it('searches commands and agents by letters in order, and Enter runs the best match', async () => {
    const run = vi.fn();
    const { dialog, onClose, names } = open(commands(run));
    const user = userEvent.setup();
    await user.keyboard('libr');
    expect(names()).toEqual(['Library']);
    await user.clear(within(dialog).getByRole('combobox'));
    await user.keyboard('shlist');
    expect(names()[0]).toBe('Show the list');
    await user.keyboard('{Enter}');
    expect(onClose).toHaveBeenCalled();
    expect(run).toHaveBeenCalledTimes(1);
    // What ran comes first next time.
    expect(readRecent()).toEqual(['cmd:run.list']);
  });

  it('shows why a command cannot run, in its row, and Enter does nothing', async () => {
    const run = vi.fn();
    const { dialog, onClose } = open(commands(run));
    const user = userEvent.setup();
    await user.keyboard('start');
    const start = within(dialog).getByRole('option', { name: 'Start team…' });
    expect(start).toHaveAttribute('aria-disabled', 'true');
    expect(start).toHaveTextContent('Fix 2 things first.');
    expect(start).toHaveAccessibleDescription('Fix 2 things first.');
    expect(start).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Enter}');
    await user.click(start);
    expect(run).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('> lists only commands and @ only agents; Backspace in the empty box shows everything again', async () => {
    const { dialog, names } = open();
    const user = userEvent.setup();
    await user.keyboard('>');
    expect(names()).toEqual(['Start team…', 'Add a group', 'Show the list', 'Library']);
    expect(within(dialog).getByRole('combobox')).toHaveValue('');
    await user.keyboard('{Backspace}@');
    expect(screen.getByRole('dialog', { name: 'Go to agent' })).toBeInTheDocument();
    expect(names()).toEqual(['Builder — idle', 'Reviewer — idle']);
    await user.keyboard('rev{Enter}');
  });

  it('jumps to an agent and remembers it as recent', async () => {
    const { onJump } = open(commands(), { scope: 'agents' });
    const user = userEvent.setup();
    await user.keyboard('rev{Enter}');
    expect(onJump).toHaveBeenCalledWith(expect.objectContaining({ id: 'b' }));
    expect(readRecent()).toEqual(['agent:b']);
  });

  it('puts what ran recently first', () => {
    localStorage.setItem('sculptured.palette.recent.v1', JSON.stringify(['agent:b', 'cmd:app.library']));
    const { dialog, names } = open();
    expect(within(dialog).getByRole('group', { name: 'Recent' })).toBeInTheDocument();
    expect(names().slice(0, 2)).toEqual(['Reviewer — idle', 'Library']);
  });
});
