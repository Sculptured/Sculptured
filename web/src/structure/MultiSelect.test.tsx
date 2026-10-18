import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { TeamDraft } from '../generated/TeamDraft';
import { forgetViewports } from '../shell/viewport';
import { draftAgent, teamDraft, teamRecord } from '../test-fixtures';
import { saveTiming } from '../useTeamDraft';
import { card, fakeTeam, pos, renderAt, resetApi, sizeViewport } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'workflow'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const lastSaved = (): TeamDraft => {
  const calls = vi.mocked(api.saveTeam).mock.calls;
  return calls[calls.length - 1][2];
};

/** Builder, Reviewer and Tester, all Claude agents. */
async function board() {
  const draft = teamDraft({
    agents: [
      draftAgent('builder', { x: 40, y: 40, kind: 'claude' }),
      draftAgent('reviewer', { x: 340, y: 40, kind: 'claude' }),
      draftAgent('tester', { x: 640, y: 40, kind: 'claude' }),
    ],
  });
  const server = fakeTeam(teamRecord({ draft }));
  renderAt('/team/t/structure');
  await screen.findByRole('region', { name: 'Design board' });
  return server;
}
const bar = () => screen.queryByRole('toolbar', { name: /agents selected$/ });
const picked = () => [...document.querySelectorAll('.sb-card.multi-selected [data-agent]')].map((e) => e.getAttribute('data-agent'));

beforeEach(() => {
  resetApi();
  forgetViewports();
  localStorage.clear();
  saveTiming.delayMs = 5;
});
afterEach(() => {
  saveTiming.delayMs = 500;
});

describe('Design board: several agents at once', () => {
  it('Shift+click adds agents to the selection without opening them; Esc lets go', async () => {
    await board();
    const user = userEvent.setup();
    expect(bar()).toBeNull();
    await user.keyboard('{Shift>}');
    await user.click(card('Builder'));
    // One is just the selection: no bar for a single agent.
    expect(bar()).toBeNull();
    await user.click(card('Tester'));
    await user.keyboard('{/Shift}');
    expect(bar()).toHaveAccessibleName('2 agents selected');
    expect(picked()).toEqual(['builder', 'tester']);
    expect(screen.queryByRole('dialog')).toBeNull();
    await user.keyboard('{Escape}');
    expect(bar()).toBeNull();
    expect(picked()).toEqual([]);
  });

  it('Ctrl+A selects every agent; one change to all of them is one undo step', async () => {
    await board();
    const user = userEvent.setup();
    await user.click(screen.getByRole('region', { name: 'Design board' }));
    await user.keyboard('{Control>}a{/Control}');
    expect(bar()).toHaveAccessibleName('3 agents selected');
    await user.click(within(bar()!).getByRole('button', { name: /Claude · model/ }));
    await user.click(screen.getByRole('button', { name: 'Codex' }));
    await waitFor(() => expect(lastSaved().agents.map((a) => a.kind)).toEqual(['codex', 'codex', 'codex']));
    await user.keyboard('{Control>}z{/Control}');
    await waitFor(() => expect(lastSaved().agents.map((a) => a.kind)).toEqual(['claude', 'claude', 'claude']));
    // Still selected after the undo.
    expect(bar()).toHaveAccessibleName('3 agents selected');
  });

  it('arrow keys and a drag move the selected agents together; the others stay', async () => {
    await board();
    const user = userEvent.setup();
    await user.keyboard('{Shift>}');
    await user.click(card('Builder'));
    await user.click(card('Reviewer'));
    await user.keyboard('{/Shift}');
    card('Reviewer').focus();
    await user.keyboard('{ArrowDown}{ArrowDown}');
    expect([pos('Builder'), pos('Reviewer'), pos('Tester')]).toEqual([[40, 72], [340, 72], [640, 40]]);
    fireEvent.pointerDown(card('Builder'), { button: 0, clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 140, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 180, clientY: 100, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 180, clientY: 100, pointerId: 1 });
    await waitFor(() => expect(lastSaved().agents.map((a) => [a.x, a.y])).toEqual([[120, 72], [420, 72], [640, 40]]));
  });

  it('Shift+drag on the empty board selects the agents inside the box; a plain drag still pans', async () => {
    await board();
    const viewport = sizeViewport();
    // From above Builder to the middle of Reviewer (world = client − 48).
    fireEvent.pointerDown(viewport, { button: 0, clientX: 60, clientY: 60, shiftKey: true, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 300, clientY: 140, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 440, clientY: 160, pointerId: 1 });
    expect(document.querySelector('.sb-marquee')).not.toBeNull();
    fireEvent.pointerUp(window, { clientX: 440, clientY: 160, pointerId: 1 });
    expect(document.querySelector('.sb-marquee')).toBeNull();
    expect(picked()).toEqual(['builder', 'reviewer']);
    // A plain press on the board lets go and pans instead.
    fireEvent.pointerDown(viewport, { button: 0, clientX: 900, clientY: 600, pointerId: 2 });
    fireEvent.pointerUp(window, { clientX: 900, clientY: 600, pointerId: 2 });
    expect(picked()).toEqual([]);
  });

  it('Delete removes them all, and Ctrl+Z brings them all back', async () => {
    await board();
    const user = userEvent.setup();
    await user.keyboard('{Shift>}');
    await user.click(card('Builder'));
    await user.click(card('Reviewer'));
    await user.keyboard('{/Shift}');
    await user.click(within(bar()!).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(lastSaved().agents.map((a) => a.id)).toEqual(['tester']));
    expect(lastSaved().connections).toEqual([]);
    expect(bar()).toBeNull();
    await user.keyboard('{Control>}z{/Control}');
    await waitFor(() => expect(lastSaved().agents.map((a) => a.id)).toEqual(['builder', 'reviewer', 'tester']));
    expect(lastSaved().connections).toHaveLength(1);
  });

  it('a new group for the selection, and a limit for each, from the bar', async () => {
    await board();
    const user = userEvent.setup();
    await user.keyboard('{Shift>}');
    await user.click(card('Reviewer'));
    await user.click(card('Tester'));
    await user.keyboard('{/Shift}');
    await user.click(within(bar()!).getByRole('button', { name: /Limits/ }));
    const form = screen.getByRole('form', { name: 'Limits for one run' });
    expect(within(form).getByRole('button', { name: 'Apply' })).toHaveAccessibleDescription('Type a limit first.');
    await user.type(within(form).getByRole('spinbutton', { name: 'Turns' }), '12');
    await user.click(within(form).getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(lastSaved().agents.map((a) => a.advanced.turn_limit?.hard ?? null)).toEqual([null, 12, 12]));
    await user.click(within(bar()!).getByRole('button', { name: /Group/ }));
    await user.click(screen.getByRole('button', { name: 'New group with these 2' }));
    await waitFor(() => {
      const d = lastSaved();
      expect(d.groups).toHaveLength(1);
      expect(d.agents.map((a) => a.group_id)).toEqual([null, d.groups[0].id, d.groups[0].id]);
    });
  });

  it('while the team runs, only moving is allowed, and the bar says why', async () => {
    const draft = teamDraft({ agents: [draftAgent('builder', { kind: 'claude' }), draftAgent('reviewer', { x: 340, kind: 'claude' })] });
    fakeTeam(teamRecord({ draft, live: true, started_revision: 1 }));
    renderAt('/team/t/structure');
    await screen.findByRole('region', { name: 'Design board' });
    const user = userEvent.setup();
    await user.click(screen.getByRole('region', { name: 'Design board' }));
    await user.keyboard('{Control>}a{/Control}');
    for (const name of ['Delete', /Group/, /Limits/, 'Folder…'])
      expect(within(bar()!).getByRole('button', { name })).toHaveAccessibleDescription('Stop the team to change this. You can still move them.');
  });
});
