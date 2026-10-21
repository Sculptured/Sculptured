import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import type { TeamDraft } from '../generated/TeamDraft';
import { forgetViewports } from '../shell/viewport';
import { draftAgent, teamDraft, teamRecord } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { saveTiming } from '../useTeamDraft';
import { card, fakeTeam, OFFSET, pos, renderAt, resetApi, sizeViewport, where } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig', 'workflow'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

const lastSaved = (): TeamDraft => {
  const calls = vi.mocked(api.saveTeam).mock.calls;
  return calls[calls.length - 1][2];
};

async function board(rec = teamRecord()) {
  const server = fakeTeam(rec);
  renderAt('/team/t/structure');
  await screen.findByRole('region', { name: 'Design board' });
  return server;
}

beforeEach(() => {
  resetApi();
  forgetViewports();
  localStorage.clear();
  // ...and every add tool, which sits under More tools until it is opened.
  localStorage.setItem('sculptured.design.more-tools.v1', 'true');
  saveTiming.delayMs = 5;
});
afterEach(() => {
  saveTiming.delayMs = 500;
});

describe('Structure board: adding agents', () => {
  it('has no side panel: selecting and dragging stay on the board; a click without dragging, or Enter, opens the agent', async () => {
    localStorage.clear();
    await board();
    const noSide = () => expect(document.querySelector('.sb-side, .side-panel, .agent-sheet, .context-panel')).toBeNull();
    noSide();
    // A newcomer sees one Add agent button; the other tools are one step back, under More tools.
    const tools = screen.getByRole('toolbar', { name: 'Add to the team' });
    expect(within(tools).getByRole('button', { name: 'Add a Claude agent' })).toHaveTextContent('Add agent');
    expect(within(tools).queryByRole('button', { name: 'Add a group' })).toBeNull();
    expect(within(tools).queryByRole('button', { name: 'Add a Codex agent' })).toBeNull();
    const more = within(tools).getByRole('button', { name: 'More tools' });
    expect(more).toHaveAttribute('aria-expanded', 'false');
    await userEvent.click(more);
    for (const name of ['Add a Codex agent', 'Add a group', 'Add the Input', 'Saved groups', 'Full text', 'Undo', 'Redo'])
      expect(within(tools).getByRole('button', { name })).toBeInTheDocument();
    expect(localStorage.getItem('sculptured.design.more-tools.v1')).toBe('true');
    await userEvent.click(within(tools).getByRole('button', { name: 'Fewer tools' }));
    expect(within(tools).queryByRole('button', { name: 'Add a group' })).toBeNull();
    expect(api.saveTeam).not.toHaveBeenCalled();
    const builder = card('Builder');
    fireEvent.pointerDown(builder, {button:0,clientX:100,clientY:100,pointerId:1});
    fireEvent.focus(builder);
    noSide();
    fireEvent.pointerMove(window, {clientX:180,clientY:260,pointerId:1});
    fireEvent.pointerUp(window, {clientX:180,clientY:260,pointerId:1});
    // The click that ends a drag only moves the card.
    fireEvent.click(builder);
    expect(pos('Builder')).toEqual([120,200]);
    noSide();
    expect(where()).toBe('/p/t/board?mode=design');
    fireEvent.keyDown(builder, {key:'Enter'});
    expect(await screen.findByRole('region', { name: 'Setup' })).toBeVisible();
    expect(where()).toBe('/p/t/agents/team.builder');
  });

  it('opens workflow input, review routes and dispatch settings on their own pages, and comes back', async () => {
    vi.mocked(api.workflow).mockResolvedValue({ config: null, run: null, active: false });
    const d = teamDraft({ agents: ['dispatcher', 'builder', 'reviewer', 'frontend'].map(id => draftAgent(id)), connections: [], workflow: {
      input: '', dispatcher: 'dispatcher', collector: 'frontend', entry: 'build', max_tasks: 24, max_steps: 12,
      stages: [{ id:'build', name:'Build', agent:'builder', routes:{ ready:'review' } }, { id:'review', name:'Review', agent:'reviewer', routes:{ revise:'build', accept:null } }],
    } });
    await board(teamRecord({ draft:d }));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name:'Input: links, notes and ideas' }));
    expect(where()).toBe('/p/t/workflow/input');
    await user.type(screen.getByLabelText('Workflow input'), 'Links and rough ideas');
    await waitFor(() => expect(lastSaved().workflow?.input).toBe('Links and rough ideas'));
    await user.click(screen.getByRole('button', { name:'Back to the board' }));
    await user.click(await screen.findByRole('button', { name:'Workflow route: revise' }));
    expect(screen.getByLabelText('Destination for revise')).toHaveValue('build');
    await user.selectOptions(screen.getByLabelText('Destination for revise'), 'review');
    await waitFor(() => expect(lastSaved().workflow?.stages[1].routes.revise).toBe('review'));
    await user.click(screen.getByRole('button', { name:'Back to the board' }));
    await user.click(await screen.findByRole('button', { name:'Dispatch N · settings' }));
    expect(screen.getByLabelText('Maximum items')).toHaveValue(24);
    expect(screen.getByRole('button', { name:'Add reusable stage' })).toBeVisible();
    expect(lastSaved().connections).toEqual([]);
  });

  it('n then Enter adds an agent at the next free place; arrows move it; Enter opens its Setup', async () => {
    await board();
    const user = userEvent.setup();
    await user.keyboard('n');
    expect(screen.getByRole('button', { name: 'Add a Claude agent' })).toHaveFocus();
    await user.keyboard('{Enter}');
    const added = card('Claude 1');
    expect(added).toHaveFocus();
    expect(added).toHaveAttribute('aria-pressed', 'true');
    // Builder sits at 40 and Reviewer at 300: the first free column slot is the third.
    expect(pos('Claude 1')).toEqual([568, 40]);
    await user.keyboard('{ArrowRight}{ArrowDown}');
    expect(pos('Claude 1')).toEqual([584, 56]);
    await user.keyboard('{Shift>}{ArrowRight}{/Shift}');
    expect(pos('Claude 1')).toEqual([648, 56]);
    await waitFor(() => expect(lastSaved().agents.find((a) => a.name === 'Claude 1')).toMatchObject({ kind: 'claude', x: 648, y: 56 }));
    await user.keyboard('{Enter}');
    expect(await screen.findByRole('textbox', { name: 'Name' })).toHaveValue('Claude 1');
  });

  it('drags an agent in from the palette and drops it where the pointer is', async () => {
    await board();
    sizeViewport();
    const claude = screen.getByRole('button', { name: 'Add a Claude agent' });
    fireEvent.pointerDown(claude, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 400, clientY: 420, pointerId: 1 });
    expect(document.querySelector('.sb-ghost')).not.toBeNull();
    fireEvent.pointerUp(window, { clientX: 500, clientY: 400, pointerId: 1 });
    // Centred under the pointer (card 200 wide, grabbed 24 below its top).
    expect(pos('Claude 1')).toEqual([500 - OFFSET - 100, 400 - OFFSET - 24]);
    expect(document.querySelector('.sb-ghost')).toBeNull();
  });
});

describe('Structure board: moving', () => {
  it('drags a card and applies one move on release, which one undo reverts', async () => {
    await board();
    const builder = card('Builder');
    fireEvent.pointerDown(builder, { button: 0, clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 140, clientY: 120, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 180, clientY: 260, pointerId: 1 });
    // While dragging the card follows the pointer; the draft is not touched until release.
    expect(api.saveTeam).not.toHaveBeenCalled();
    fireEvent.pointerUp(window, { clientX: 180, clientY: 260, pointerId: 1 });
    expect(pos('Builder')).toEqual([120, 200]);
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(1));
    expect(lastSaved().agents[0]).toMatchObject({ x: 120, y: 200 });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Undo' }));
    expect(pos('Builder')).toEqual([40, 40]);
  });

  it('selects with Tab and moves with the arrow keys; consecutive moves are one undo step', async () => {
    await board();
    const user = userEvent.setup();
    act(() => card('Builder').focus());
    expect(card('Builder')).toHaveAttribute('aria-pressed', 'true');
    await user.keyboard('{ArrowDown}{ArrowDown}{ArrowDown}');
    expect(pos('Builder')).toEqual([40, 88]);
    await user.keyboard('{Control>}z{/Control}');
    expect(pos('Builder')).toEqual([40, 40]);
    await user.keyboard('{Control>}{Shift>}z{/Shift}{/Control}');
    expect(pos('Builder')).toEqual([40, 88]);
    await user.keyboard('{Escape}');
    expect(card('Builder')).toHaveAttribute('aria-pressed', 'false');
  });
});

describe('Structure board: connecting', () => {
  const unconnected = () => teamRecord({ draft: teamDraft({ connections: [] }) });

  it('drags from an agent’s ● port to another agent', async () => {
    await board(unconnected());
    const port = card('Builder').parentElement!.querySelector('.port.out')!;
    fireEvent.pointerDown(port, { button: 0, clientX: 288, clientY: 188, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 350, clientY: 190, pointerId: 1 });
    expect(document.querySelector('.edge.wire')).not.toBeNull();
    // Reviewer's card spans 300–500 × 40–240 in the world.
    fireEvent.pointerUp(window, { clientX: 300 + OFFSET + 50, clientY: 40 + OFFSET + 50, pointerId: 1 });
    const label = await screen.findByRole('button', { name: 'Builder hands work to Reviewer' });
    expect(label).toHaveTextContent('hands work to');
    await waitFor(() => expect(lastSaved().connections).toEqual([expect.objectContaining({ from: 'builder', to: 'reviewer', kind: 'hands-work-to' })]));
  });

  it("connects from the agent's Setup with Connect to…, by keyboard", async () => {
    await board(unconnected());
    const user = userEvent.setup();
    await user.click(card('Builder'));
    const links = await screen.findByRole('region', { name: 'Connections' });
    await user.click(within(links).getByRole('button', { name: 'Connect to…' }));
    const menu = screen.getByRole('menu', { name: 'Builder hands work to' });
    expect(within(menu).getAllByRole('menuitem')[0]).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(within(links).getByRole('combobox', { name: 'How Builder is linked to Reviewer' })).toHaveValue('hands-work-to');
    await user.keyboard('{Escape}');
    expect(await screen.findByRole('button', { name: 'Builder hands work to Reviewer' })).toBeInTheDocument();
  });

  it('refuses a connection to itself or a duplicate, with the reason in words', async () => {
    await board();
    const port = card('Builder').parentElement!.querySelector('.port.out')!;
    fireEvent.pointerDown(port, { button: 0, clientX: 288, clientY: 188, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 400, clientY: 190, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 300 + OFFSET + 50, clientY: 40 + OFFSET + 50, pointerId: 1 });
    expect(await screen.findByText('Builder already hands work to Reviewer.')).toBeInTheDocument();
  });
});

describe('Structure board: groups', () => {
  it('hides a group and its internal connections without saving or moving the board', async () => {
    await board(teamRecord({draft:teamDraft({groups:[{id:'g',label:'Workers',x:10,y:10,w:570,h:300}], agents:[draftAgent('builder',{group_id:'g'}),draftAgent('reviewer',{group_id:'g',x:300})]})}));
    const user=userEvent.setup(), world=screen.getByTestId('structure-world');
    const transform=world.style.transform, original=pos('Builder');
    await user.click(screen.getByRole('button',{name:'Hide contents of Workers'}));
    expect(screen.queryByRole('button',{name:/^Builder — /})).toBeNull();
    expect(screen.queryByRole('button',{name:'Builder hands work to Reviewer'})).toBeNull();
    expect(world.querySelectorAll('path.edge')).toHaveLength(0);
    expect(world.style.transform).toBe(transform);
    expect(screen.getByRole('button',{name:'Show contents of Workers'})).toHaveAttribute('aria-expanded','false');
    await user.keyboard('{Enter}');
    expect(pos('Builder')).toEqual(original);
    expect(screen.getByRole('button',{name:'Builder hands work to Reviewer'})).toBeInTheDocument();
    expect(api.saveTeam).not.toHaveBeenCalled();
  });

  it('+ Group, then Move to group… puts an agent in it by keyboard; the group can be renamed', async () => {
    await board();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Add a group' }));
    const head = screen.getByRole('button', { name: /^Group: Group 1/ });
    expect(head).toHaveFocus();
    // The other agents stay outside the new group: no main group is drawn around them.
    expect(screen.queryByRole('button', { name: /^Group: Main group/ })).toBeNull();
    // A group's settings are a small bar over the board, not a side panel.
    const gbar = screen.getByRole('toolbar', { name: 'Group' });
    await user.clear(within(gbar).getByRole('textbox', { name: 'Group name' }));
    await user.type(within(gbar).getByRole('textbox', { name: 'Group name' }), 'Review');
    expect(screen.getByRole('button', { name: /^Group: Review/ })).toBeInTheDocument();
    await user.click(card('Reviewer'));
    const setup = await screen.findByRole('region', { name: 'Setup' });
    await user.click(setup.querySelector('.setup-advanced summary')!);
    await user.click(within(setup).getByRole('button', { name: 'Move to group…' }));
    await user.click(screen.getByRole('menuitem', { name: 'Review' }));
    // Its address follows it into the group.
    await waitFor(() => expect(where()).toMatch(/^\/p\/t\/agents\/group-[a-z0-9-]+\.reviewer/));
    expect(within(await screen.findByRole('region', { name: 'Setup' })).getByText('Review', { selector: '.fld-value' })).toBeInTheDocument();
    await waitFor(() => {
      const d = lastSaved();
      const g = d.groups.find((x) => x.label === 'Review')!;
      expect(d.agents.find((a) => a.id === 'reviewer')!.group_id).toBe(g.id);
    });
  });

  it('dropping a card on a group by pointer moves it into that group', async () => {
    await board();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Add a group' }));
    const box = screen.getByRole('button', { name: /^Group: Group 1/ }).closest('.sb-group') as HTMLElement;
    const g = { x: parseFloat(box.style.left), y: parseFloat(box.style.top), w: parseFloat(box.style.width), h: parseFloat(box.style.height) };
    const [rx, ry] = pos('Reviewer');
    const dx = g.x + g.w / 2 - 100 - rx;
    const dy = g.y + g.h / 2 - 100 - ry;
    fireEvent.pointerDown(card('Reviewer'), { button: 0, clientX: 400, clientY: 200, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 400 + dx / 2, clientY: 200 + dy / 2, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 400 + dx, clientY: 200 + dy, pointerId: 1 });
    expect(box).toHaveClass('drop-target');
    fireEvent.pointerUp(window, { clientX: 400 + dx, clientY: 200 + dy, pointerId: 1 });
    await waitFor(() => {
      const d = lastSaved();
      expect(d.agents.find((a) => a.id === 'reviewer')!.group_id).toBe(d.groups.find((x) => x.label === 'Group 1')!.id);
      expect(d.agents.find((a) => a.id === 'builder')!.group_id).toBeNull();
    });
  });

  it('moves a group by dragging its head and by the arrow keys', async () => {
    await board();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Add a group' }));
    const head = screen.getByRole('button', { name: /^Group: Group 1/ });
    const box = head.closest('.sb-group') as HTMLElement;
    const x0 = parseFloat(box.style.left);
    await user.keyboard('{ArrowRight}');
    expect(parseFloat(box.style.left)).toBe(x0 + 16);
    fireEvent.pointerDown(head, { button: 0, clientX: 600, clientY: 60, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 640, clientY: 140, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 640, clientY: 140, pointerId: 1 });
    expect(parseFloat(box.style.left)).toBe(x0 + 16 + 40);
  });
});

describe('Structure board: deleting and refusals', () => {
  it('Del removes the selected agent and its connections; undo brings both back', async () => {
    await board();
    const user = userEvent.setup();
    act(() => card('Reviewer').focus());
    await user.keyboard('{Delete}');
    expect(screen.queryByRole('button', { name: /^Reviewer — / })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Builder hands work to Reviewer' })).toBeNull();
    await user.keyboard('{Control>}z{/Control}');
    expect(card('Reviewer')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Builder hands work to Reviewer' })).toBeInTheDocument();
  });

  it('deletes a whole group by keyboard and undo restores it', async () => {
    const grp = (id: string, x: number) => ({ id, label: id === 'team' ? 'Main group' : 'Side', x, y: 0, w: 248, h: 276 });
    const d = teamDraft({
      groups: [grp('team', 16), grp('side', 300)],
      agents: [draftAgent('builder', { x: 40, y: 48, group_id: 'team' }), draftAgent('reviewer', { x: 328, y: 48, group_id: 'side' })],
    });
    await board(teamRecord({ draft: d }));
    const user = userEvent.setup();
    act(() => screen.getByRole('button', { name: /^Group: Side/ }).focus());
    await user.keyboard('{Delete}');
    expect(screen.queryByRole('button', { name: /^Group: Side/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'Undo' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'Undo' }));
    expect(screen.getByRole('button', { name: /^Group: Side/ })).toBeVisible();
    expect(card('Reviewer')).toBeVisible();
  });
});

describe('Structure board: autosave', () => {
  it('shows saving… then saved, and on a conflict keeps your edits and offers Reload', async () => {
    saveTiming.delayMs = 150;
    const server = await board();
    const user = userEvent.setup();
    await user.click(card('Builder'));
    const name = await screen.findByRole('textbox', { name: 'Name' });
    await user.type(name, 'X');
    expect(document.querySelector('.save-state')).toHaveTextContent('saving…');
    await waitFor(() => expect(document.querySelector('.save-state')).toHaveTextContent('saved'));
    // Saved is the normal state: nothing on screen, still announced.
    expect(document.querySelector('.save-state')).toHaveClass('sr-only');
    expect(document.querySelector('.save-state')).toHaveAttribute('role', 'status');
    vi.mocked(api.saveTeam).mockRejectedValue(new ApiError(409, 'draft_conflict', 'This team was changed somewhere else (it is now at version 9).'));
    await user.type(name, 'Y');
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('not saved: This team was changed somewhere else');
    expect(name).toHaveValue('BuilderXY');
    expect(screen.getByRole('heading', { level: 1, name: 'BuilderXY' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Start team' })).toBeDisabled();
    server.rec = teamRecord({ draft: teamDraft({ revision: 9, name: 'Their team' }) });
    await user.click(within(alert).getByRole('button', { name: 'Reload' }));
    await waitFor(() => expect(document.querySelector('.save-state')).toHaveTextContent('saved'));
    await user.click(screen.getByRole('button', { name: /^Board/ }));
    expect(await screen.findByRole('button', { name: /^Builder — / })).toBeInTheDocument();
  });
});

describe('Structure board: problems and Start', () => {
  const issue = (kind: 'agent' | 'connection', id: string, message: string) => ({
    element: { kind, id },
    severity: 'error' as const,
    code: 'x',
    message,
    detail: null,
    spec_path: null,
  });

  it('pins problems to their element, lists them, and Start says why it is disabled', async () => {
    await board(
      teamRecord({
        issues: [issue('agent', 'reviewer', 'Reviewer needs a folder to work in'), issue('connection', 'c1', "Builder's connection loops back to itself")],
      }),
    );
    const reviewer = card('Reviewer');
    expect(reviewer).toHaveAccessibleName('Reviewer — Practice, 1 thing to fix');
    expect(reviewer).toHaveAccessibleDescription('Reviewer needs a folder to work in');
    expect(reviewer).toHaveTextContent('▲');
    expect(screen.getByRole('button', { name: /^Builder hands work to Reviewer, 1 to fix/ })).toHaveTextContent('▲');
    const start = screen.getByRole('button', { name: 'Start team' });
    expect(start).toBeDisabled();
    expect(start).toHaveAccessibleDescription(
      "Start team: 2 things to fix: Reviewer needs a folder to work in; Builder's connection loops back to itself",
    );
    // The status beside Start says it in short; the full reason is its description.
    expect(screen.getByText('Draft · 2 to fix')).toBeInTheDocument();
    const fixes = screen.getByRole('region', { name: 'Things to fix' });
    // An agent's problem opens that agent's Setup, where it is fixed.
    await userEvent.setup().click(within(fixes).getByRole('button', { name: /Reviewer needs a folder/ }));
    const setup = await screen.findByRole('region', { name: 'Setup' });
    expect(screen.getByRole('heading', { level: 1, name: 'Reviewer' })).toBeInTheDocument();
    expect(within(setup).getByRole('status', { name: 'Problems' })).toHaveTextContent('2 to fix');
  });

  it('a missing project folder is fixed from Things to fix: Choose folder… picks it right there', async () => {
    const team = { element: { kind: 'team' as const, id: 't' }, severity: 'error' as const, code: 'project-folder-needed', message: 'Choose the project folder: the guard and Tests run there.', detail: null, spec_path: null };
    await board(teamRecord({ issues: [team] }));
    const user = userEvent.setup();
    const fixes = screen.getByRole('region', { name: 'Things to fix' });
    await user.click(within(fixes).getByRole('button', { name: 'Choose folder…' }));
    const picker = await screen.findByRole('dialog');
    await user.click(await within(picker).findByRole('button', { name: /^Use/ }));
    await waitFor(() => expect(lastSaved().folder).toBe('/home/me'));
  });

  it('a start refused with problems shows them on the board', async () => {
    await board();
    vi.mocked(api.startTeam).mockRejectedValue(
      new ApiError(422, 'team_invalid', '1 thing to fix', undefined, [issue('agent', 'builder', 'Builder needs a folder to work in')]),
    );
    await userEvent.setup().click(screen.getByRole('button', { name: 'Start team' }));
    const dialog = await screen.findByRole('alertdialog', { name: 'The team could not start' });
    expect(dialog).toHaveTextContent('1 thing to fix');
    expect(card('Builder')).toHaveAccessibleName('Builder — Practice, 1 thing to fix');
  });

  // Hardening item 1: agent folders must sit inside home. The server's issues name the agent in plain words.
  it.each([
    ['folder-outside-home', '/etc/secret', "Builder's folder is outside your home folder", 'outside your home folder'],
    ['folder-unreadable', '/home/me/locked', "Sculptured can't read Builder's folder: /home/me/locked", "can't be read"],
    ['folder-bad-path', '/home/me/a/../b', "Builder's folder path can't be used: /home/me/a/../b", "path can't be used"],
  ])('%s shows in plain words on the agent card, its panel and Things to fix', async (code, folder, message, short) => {
    const rec = teamRecord({
      draft: teamDraft({ agents: [draftAgent('builder', { x: 40, y: 40, folder }), draftAgent('reviewer', { x: 300, y: 40 })] }),
      issues: [{ element: { kind: 'agent', id: 'builder' }, severity: 'error', code, message, detail: 'technical detail', spec_path: 'pods[0].members[0].cwd' }],
    });
    await board(rec);
    const builder = card('Builder');
    expect(builder).toHaveAccessibleName('Builder — Practice, 1 thing to fix');
    expect(builder).toHaveAccessibleDescription(message);
    const line = builder.querySelector('.sb-folder')!;
    expect(line).toHaveTextContent(short);
    expect(line).toHaveClass('tone-red');
    expect(screen.getByRole('button', { name: 'Start team' })).toBeDisabled();
    const fixes = screen.getByRole('region', { name: 'Things to fix' });
    await userEvent.setup().click(within(fixes).getByRole('button', { name: message }));
    const setup = await screen.findByRole('region', { name: 'Setup' });
    expect(within(setup).getByRole('status', { name: 'Problems' })).toHaveTextContent(/1 to fix:\s*Folder/);
    expect(within(setup).getByRole('region', { name: 'Folder' })).toHaveTextContent(message);
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
  });
});

describe('Structure board: words', () => {
  it('uses no OpenRig words outside Advanced, with panels, menus and the folder picker open', async () => {
    await board();
    const user = userEvent.setup();
    const app = () => document.querySelector<HTMLElement>('.app')!;
    expect(openRigWords(app())).toEqual([]);
    await user.click(card('Builder'));
    await user.click(await screen.findByRole('button', { name: 'Connect to…' }));
    expect(openRigWords(app())).toEqual([]);
    await user.keyboard('{Escape}');
    await user.click(screen.getByRole('button', { name: 'Choose a folder for Builder' }));
    await screen.findByRole('dialog', { name: 'Choose a folder' });
    expect(openRigWords(app())).toEqual([]);
    await user.keyboard('{Escape}');
    await user.click(screen.getByRole('button', { name: /^Board/ }));
    await user.click(await screen.findByRole('button', { name: 'Builder hands work to Reviewer' }));
    expect(screen.getByRole('toolbar', { name: 'Connection' })).toBeInTheDocument();
    expect(openRigWords(app())).toEqual([]);
    await user.click(screen.getByRole('button', { name: 'Add a group' }));
    expect(openRigWords(app())).toEqual([]);
  });
});

describe('Structure board: viewport', () => {
  it('keeps zoom across edits, and fits again on f', async () => {
    await board();
    const user = userEvent.setup();
    const world = () => screen.getByTestId('structure-world').style.transform;
    await user.click(screen.getByRole('button', { name: 'Zoom in' }));
    const zoomed = world();
    expect(zoomed).toContain('scale(1.2)');
    act(() => card('Builder').focus());
    await user.keyboard('{ArrowDown}');
    expect(world()).toBe(zoomed);
  });
});


describe('Direct arrow and collapsed group editing', () => {
  it('retargets an arrow on drop, preserves identity, and supports undo and keyboard editing', async () => {
    await board(teamRecord({draft:teamDraft({agents:[draftAgent('builder',{x:40,y:40}),draftAgent('reviewer',{x:300,y:40}),draftAgent('third',{x:560,y:40})]})}));
    sizeViewport();
    fireEvent.click(document.querySelector('.edge-hit')!);
    const handle=screen.getByRole('button',{name:/Change destination of/});
    fireEvent.pointerDown(handle,{button:0,clientX:350,clientY:100,pointerId:2});
    fireEvent.pointerMove(window,{clientX:650,clientY:100,pointerId:2});
    fireEvent.pointerUp(window,{clientX:650,clientY:100,pointerId:2});
    await waitFor(()=>expect(lastSaved().connections).toEqual([{id:'c1',from:'builder',to:'third',kind:'hands-work-to'}]));
    await userEvent.click(screen.getByRole('button',{name:'Undo'}));
    await waitFor(()=>expect(lastSaved().connections[0].to).toBe('reviewer'));
    fireEvent.click(screen.getByRole('button',{name:/Change destination of/}),{detail:0});
    await userEvent.selectOptions(screen.getByLabelText('Destination agent'),'third');
    await waitFor(()=>expect(lastSaved().connections[0].to).toBe('third'));
  });

  it('moves a collapsed group and all its members by dragging its body without panning', async () => {
    localStorage.clear();
    await board(teamRecord({draft:teamDraft({groups:[{id:'g',label:'Workers',x:0,y:0,w:560,h:300}],agents:[draftAgent('builder',{x:40,y:40,group_id:'g'}),draftAgent('reviewer',{x:300,y:40,group_id:'g'})]})}));
    await userEvent.click(screen.getByRole('button',{name:'Hide contents of Workers'}));
    const body=document.querySelector('.group-collapsed')!, transform=screen.getByTestId('structure-world').style.transform;
    fireEvent.pointerDown(body,{button:0,clientX:300,clientY:180,pointerId:1});
    fireEvent.pointerMove(window,{clientX:380,clientY:260,pointerId:1});
    fireEvent.pointerUp(window,{clientX:380,clientY:260,pointerId:1});
    await waitFor(()=>expect(lastSaved().agents[0]).toMatchObject({x:120,y:120,group_id:'g'}));
    expect(lastSaved().agents[1]).toMatchObject({x:380,y:120,group_id:'g'});
    expect(screen.getByTestId('structure-world').style.transform).toBe(transform);
    expect(screen.queryByRole('button',{name:/Builder —/})).not.toBeInTheDocument();
  });

  it('drags workflow branch destinations without adding ordinary connections', async () => {
    vi.mocked(api.workflow).mockResolvedValue({config:null,run:null,active:false});
    await board(teamRecord({draft:teamDraft({agents:[draftAgent('builder',{x:40,y:40}),draftAgent('reviewer',{x:300,y:40}),draftAgent('frontend',{x:560,y:40})],connections:[],workflow:{input:'',dispatcher:'builder',collector:'frontend',entry:'review',max_tasks:24,max_steps:12,stages:[{id:'review',name:'Review',agent:'reviewer',routes:{revise:'review',accept:null}}]}})}));
    sizeViewport();
    fireEvent.click(document.querySelector('[data-workflow-route="review:revise"]')!);
    const handle=screen.getByRole('button',{name:'Change destination of revise'});
    fireEvent.pointerDown(handle,{button:0,clientX:380,clientY:100,pointerId:2});
    fireEvent.pointerMove(window,{clientX:650,clientY:100,pointerId:2});
    fireEvent.pointerUp(window,{clientX:650,clientY:100,pointerId:2});
    await waitFor(()=>expect(lastSaved().workflow?.stages[0].routes.revise).toBeNull());
    expect(lastSaved().connections).toEqual([]);
    await userEvent.click(screen.getByRole('button',{name:'Undo'}));
    await waitFor(()=>expect(lastSaved().workflow?.stages[0].routes.revise).toBe('review'));
  });
});

it('does not auto-fit the viewport after a drag changes the drawing bounds', async () => {
  localStorage.clear();
  const width=vi.spyOn(HTMLElement.prototype,'clientWidth','get').mockReturnValue(1000);
  const height=vi.spyOn(HTMLElement.prototype,'clientHeight','get').mockReturnValue(800);
  try {
    await board();
    const transform=screen.getByTestId('structure-world').style.transform;
    fireEvent.pointerDown(card('Reviewer'),{button:0,clientX:600,clientY:300,pointerId:1});
    fireEvent.pointerMove(window,{clientX:900,clientY:500,pointerId:1});
    fireEvent.pointerUp(window,{clientX:900,clientY:500,pointerId:1});
    await waitFor(()=>expect(api.saveTeam).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId('structure-world').style.transform).toBe(transform);
  } finally { width.mockRestore(); height.mockRestore(); }
});

it('shows every card in full on request, and back', async()=>{
  localStorage.clear();
  await board(teamRecord({draft:teamDraft()}));
  await userEvent.click(screen.getByRole('button',{name:'More tools'}));
  const toggle=screen.getByRole('button',{name:'Full text'});
  expect(toggle).toHaveAttribute('aria-pressed','false');
  await userEvent.click(toggle);
  expect(toggle).toHaveAttribute('aria-pressed','true');
  expect(document.querySelectorAll('.sb-slot.full-text').length).toBeGreaterThan(0);
  await userEvent.click(toggle);
  expect(document.querySelectorAll('.sb-slot.full-text').length).toBe(0);
});

it('sets the folder for every agent from the board toolbar', async()=>{
  await board(teamRecord({draft:teamDraft({folder:'/home/me/proj'})}));
  const button=screen.getByRole('button',{name:/^Project folder: .*proj/});
  await userEvent.click(button);
  expect(screen.getByRole('dialog')).toBeInTheDocument();
});

it('drags a stage box like Input, saving its position, and moves it by keyboard', async()=>{
  localStorage.clear();vi.mocked(api.workflow).mockResolvedValue({config:null,run:null,active:false});
  const d=teamDraft({connections:[],workflow:{input:'',dispatcher:'builder',collector:'reviewer',entry:'build',max_tasks:24,max_steps:12,stages:[
    {id:'build',name:'Build',agent:'builder',routes:{done:'chief'}},{id:'chief',name:'Sign-off',agent:'',kind:'human',prompt:'Ship it?',routes:{ship:null,rework:'build'},position:{x:600,y:400}}]}});
  await board(teamRecord({draft:d}));sizeViewport();
  const box=screen.getByRole('button',{name:/^Person decides: Sign-off/});
  fireEvent.pointerDown(box,{button:0,clientX:100,clientY:100,pointerId:1});
  fireEvent.pointerMove(window,{clientX:180,clientY:260,pointerId:1});fireEvent.pointerUp(window,{clientX:180,clientY:260,pointerId:1});fireEvent.click(box);
  await waitFor(()=>expect(lastSaved().workflow?.stages[1].position).toEqual({x:680,y:560}));
  fireEvent.keyDown(box,{key:'ArrowRight',shiftKey:true});
  await waitFor(()=>expect(lastSaved().workflow?.stages[1].position).toEqual({x:744,y:560}));
});

it('drags Input without opening its editor, saves the position and supports keyboard movement and undo', async()=>{
  localStorage.clear();vi.mocked(api.workflow).mockResolvedValue({config:null,run:null,active:false});
  const d=teamDraft({connections:[],workflow:{input:'Keep these notes',dispatcher:'builder',collector:'reviewer',entry:'build',max_tasks:24,max_steps:12,stages:[{id:'build',name:'Build',agent:'builder',routes:{done:null}}]}});
  await board(teamRecord({draft:d}));sizeViewport();
  const input=screen.getByRole('button',{name:'Input: links, notes and ideas'}),world=screen.getByTestId('structure-world').style.transform;
  fireEvent.pointerDown(input,{button:0,clientX:100,clientY:100,pointerId:1});fireEvent.focus(input);
  fireEvent.pointerMove(window,{clientX:180,clientY:260,pointerId:1});fireEvent.pointerUp(window,{clientX:180,clientY:260,pointerId:1});fireEvent.click(input);
  await waitFor(()=>expect(lastSaved().workflow?.input_position).toEqual({x:-200,y:200}));
  expect(screen.queryByLabelText('Workflow input')).not.toBeInTheDocument();expect(lastSaved().workflow?.input).toBe('Keep these notes');expect(screen.getByTestId('structure-world').style.transform).toBe(world);
  fireEvent.keyDown(input,{key:'ArrowRight',shiftKey:true});
  await waitFor(()=>expect(lastSaved().workflow?.input_position).toEqual({x:-136,y:200}));
  await userEvent.click(screen.getByRole('button',{name:'Undo'}));
  await waitFor(()=>expect(lastSaved().workflow?.input_position).toEqual({x:-200,y:200}));
  fireEvent.pointerDown(input,{button:0,clientX:100,clientY:100,pointerId:2});fireEvent.pointerMove(window,{clientX:180,clientY:260,pointerId:2});fireEvent.pointerCancel(window,{pointerId:2});
  expect(input.style.left).toBe('-200px');
  fireEvent.click(input,{detail:0});expect(screen.getByLabelText('Workflow input')).toHaveValue('Keep these notes');
});
describe('Structure board: copy, paste and right-click', () => {
  it('Ctrl+C then Ctrl+V pastes a copy of the selected agent, selected and saved; Ctrl+D duplicates', async () => {
    await board();
    const user = userEvent.setup();
    act(() => card('Builder').focus());
    await user.keyboard('{Control>}c{/Control}');
    await user.keyboard('{Control>}v{/Control}');
    expect(card('Builder 2')).toHaveFocus();
    await user.keyboard('{Control>}d{/Control}');
    expect(card('Builder 3')).toHaveFocus();
    await waitFor(() => expect(lastSaved().agents.map((a) => a.name)).toEqual(['Builder', 'Reviewer', 'Builder 2', 'Builder 3']));
    // A copied agent has no arrows of its own.
    expect(lastSaved().connections).toHaveLength(1);
  });

  it('copies a whole group with its agents and arrow, and pastes it as a new group', async () => {
    const d = teamDraft({
      groups: [{ id: 'pair', label: 'Pair', x: 16, y: 0, w: 560, h: 280 }],
      agents: [draftAgent('builder', { x: 40, y: 52, group_id: 'pair' }), draftAgent('reviewer', { x: 300, y: 52, group_id: 'pair' })],
    });
    await board(teamRecord({ draft: d }));
    const user = userEvent.setup();
    act(() => screen.getByRole('button', { name: /^Group: Pair/ }).focus());
    await user.keyboard('{Control>}c{/Control}{Control>}v{/Control}');
    expect(screen.getByRole('button', { name: /^Group: Pair 2/ })).toHaveFocus();
    await waitFor(() => {
      const s = lastSaved();
      const copy = s.groups.find((g) => g.label === 'Pair 2')!;
      const members = s.agents.filter((a) => a.group_id === copy.id);
      expect(members).toHaveLength(2);
      expect(s.connections.some((c) => c.from === members[0].id && c.to === members[1].id)).toBe(true);
    });
  });

  it('right-click on empty board adds an agent there or a group; on a group, Ungroup keeps its agents', async () => {
    await board();
    sizeViewport();
    const user = userEvent.setup();
    const viewport = screen.getByTestId('structure-viewport');
    fireEvent.contextMenu(viewport, { clientX: 500, clientY: 600 });
    await user.click(within(screen.getByRole('menu', { name: 'Board actions' })).getByRole('menuitem', { name: /Add a Claude agent here/ }));
    expect(pos('Claude 1')).toEqual([500 - OFFSET - 100, 600 - OFFSET - 24]);
    fireEvent.contextMenu(viewport, { clientX: 900, clientY: 100 });
    await user.click(screen.getByRole('menuitem', { name: 'New group here' }));
    const head = screen.getByRole('button', { name: /^Group: Group 1/ });
    fireEvent.contextMenu(head, { clientX: 905, clientY: 105 });
    const menu = screen.getByRole('menu', { name: 'Group 1 actions' });
    await user.click(within(menu).getByRole('menuitem', { name: /Add a Codex agent here/ }));
    await waitFor(() => expect(lastSaved().agents.find((a) => a.name === 'Codex 1')?.group_id).toBe(lastSaved().groups[0]?.id));
    fireEvent.contextMenu(screen.getByRole('button', { name: /^Group: Group 1/ }), { clientX: 905, clientY: 105 });
    await user.click(screen.getByRole('menuitem', { name: /Ungroup/ }));
    expect(screen.queryByRole('button', { name: /^Group: Group 1/ })).toBeNull();
    await waitFor(() => expect(lastSaved()).toMatchObject({ groups: [] }));
    expect(lastSaved().agents.find((a) => a.name === 'Codex 1')!.group_id).toBeNull();
  });

  it('right-click on an agent offers Copy, Duplicate and Claude or Codex (Practice only for an agent that already is one)', async () => {
    await board();
    const user = userEvent.setup();
    fireEvent.contextMenu(card('Reviewer'), { clientX: 200, clientY: 200 });
    const menu = screen.getByRole('menu', { name: 'Reviewer actions' });
    expect(within(menu).getByRole('menuitemradio', { name: /Runs on Practice/ })).toBeChecked();
    expect(within(menu).getByRole('menuitem', { name: /^Copy/ })).toBeInTheDocument();
    await user.click(within(menu).getByRole('menuitemradio', { name: /Runs on Claude/ }));
    fireEvent.contextMenu(card('Reviewer'), { clientX: 200, clientY: 200 });
    expect(screen.queryByRole('menuitemradio', { name: /Runs on Practice/ })).toBeNull();
    await user.click(screen.getByRole('menuitem', { name: /Duplicate/ }));
    expect(card('Reviewer 2')).toBeInTheDocument();
  });
});

describe('Structure board: the Input box', () => {
  it('Input on the bar adds it pointing to where the work starts; its editor writes the text and picks who gets it', async () => {
    await board();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Add the Input' }));
    const editor = screen.getByRole('dialog', { name: 'Input' });
    await user.type(within(editor).getByRole('textbox', { name: 'Input text' }), 'Ship the landing page');
    await user.click(within(editor).getByRole('checkbox', { name: 'Reviewer' }));
    await waitFor(() => expect(lastSaved().input).toMatchObject({ text: 'Ship the landing page', to: ['builder', 'reviewer'] }));
    expect(screen.queryByRole('button', { name: 'Add the Input' })).toBeNull();
    expect(screen.getByRole('button', { name: /^Input: Ship the landing page\. Goes to Builder, Reviewer/ })).toBeInTheDocument();
    await user.click(within(editor).getByRole('button', { name: 'Remove the Input' }));
    await waitFor(() => expect(lastSaved().input).toBeUndefined());
  });

  it('Add files… picks files on the computer and adds their paths to the Input for the agents to open', async () => {
    await board();
    vi.mocked(api.listFolders).mockResolvedValue({ path: '/home/me', parent: null, home: '/home/me', entries: [], truncated: false, files: [{ name: 'document_example.pdf', path: '/home/me/document_example.pdf' }] });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Add the Input' }));
    const editor = screen.getByRole('dialog', { name: 'Input' });
    await user.type(within(editor).getByRole('textbox', { name: 'Input text' }), 'Read the manual');
    await user.click(within(editor).getByRole('button', { name: 'Add files…' }));
    await user.click(await screen.findByRole('checkbox', { name: 'document_example.pdf' }));
    await user.click(screen.getByRole('button', { name: 'Add 1 file' }));
    await waitFor(() => expect(lastSaved().input?.text).toBe('Read the manual\n/home/me/document_example.pdf'));
  });

  it('drags in from the bar to where it is dropped, moves by keys, and Delete removes it', async () => {
    await board();
    sizeViewport();
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Add the Input' }), { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 500, clientY: 500, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 500, clientY: 500, pointerId: 1 });
    await waitFor(() => expect(lastSaved().input).toMatchObject({ x: 500 - OFFSET - 100, y: 500 - OFFSET - 24 }));
    const node = screen.getByRole('button', { name: /^Input: empty/ });
    act(() => node.focus());
    const user = userEvent.setup();
    await user.keyboard('{ArrowRight}');
    await waitFor(() => expect(lastSaved().input!.x).toBe(500 - OFFSET - 100 + 16));
    await user.keyboard('{Delete}');
    await waitFor(() => expect(lastSaved().input).toBeUndefined());
  });

  it('right-click on the board offers Add the Input here', async () => {
    await board();
    sizeViewport();
    fireEvent.contextMenu(screen.getByTestId('structure-viewport'), { clientX: 400, clientY: 500 });
    await userEvent.setup().click(screen.getByRole('menuitem', { name: 'Add the Input here' }));
    await waitFor(() => expect(lastSaved().input).toMatchObject({ x: 400 - OFFSET, y: 500 - OFFSET }));
  });
});

describe('Structure board: less on screen, nothing removed', () => {
  it('cards show name, a muted kind line, the job and the folder; no JOB label and no footer', async () => {
    await board();
    const builder = card('Builder').closest('.sb-card')!;
    expect(builder.querySelector('.sb-sec')).toBeNull();
    expect(builder.querySelector('.box-foot')).toBeNull();
    expect(builder).not.toHaveTextContent('no connections out');
    expect(builder).not.toHaveTextContent(/hands work to/);
    expect(builder.querySelector('.sb-kind')).toHaveTextContent(/^Practice · free$/);
    // Who it hands work to is still on the board (the arrow's label) and in its panel.
    expect(screen.getByRole('button', { name: /Builder hands work to Reviewer/ })).toBeInTheDocument();
  });

  it('shows the line legend only while a dashed line is drawn; ? always has it', async () => {
    const d = teamDraft();
    await board(teamRecord({ draft: d }));
    expect(document.querySelector('.sb-legend')).toBeNull();
    const user = userEvent.setup();
    await user.keyboard('?');
    const help = screen.getByRole('dialog', { name: 'Keys' });
    expect(within(help).getByRole('region', { name: 'Lines on the board' })).toHaveTextContent('reports to / watches (not enforced)');
    await user.keyboard('{Escape}');
  });

  it('draws the legend when the team has a reports-to or watches line', async () => {
    const d = teamDraft({ connections: [{ id: 'c1', from: 'builder', to: 'reviewer', kind: 'hands-work-to' }, { id: 'c2', from: 'reviewer', to: 'builder', kind: 'reports-to' }] });
    await board(teamRecord({ draft: d }));
    expect(document.querySelector('.sb-legend')).toHaveTextContent('hands work to (it starts after)');
    expect(document.querySelector('.sb-legend')).toHaveTextContent('reports to / watches (not enforced)');
  });

  it('Run, before the first start, says why on hover or focus, not as a printed line', async () => {
    await board();
    const modes = screen.getByRole('group', { name: 'Board mode' });
    expect(within(modes).getByRole('button', { name: /Design/ })).toHaveAttribute('aria-pressed', 'true');
    const run = within(modes).getByRole('button', { name: /^Run/ });
    expect(run).toBeDisabled();
    expect(run).toHaveAttribute('data-tip', 'Run opens once the team has started');
    expect(run).toHaveAccessibleDescription('Run opens once the team has started');
  });

  it('the workflow list is a quiet board tool and a Ctrl K command, and opens on its own page', async () => {
    vi.mocked(api.workflow).mockResolvedValue({ config: null, run: null, active: false });
    const d = teamDraft({ agents: ['dispatcher', 'builder', 'reviewer', 'frontend'].map(id => draftAgent(id)), connections: [], workflow: {
      input: '', dispatcher: 'dispatcher', collector: 'frontend', entry: 'build', max_tasks: 24, max_steps: 12,
      stages: [{ id:'build', name:'Build', agent:'builder', routes:{ ready:'review' } }, { id:'review', name:'Review', agent:'reviewer', routes:{ revise:'build', accept:null } }],
    } });
    await board(teamRecord({ draft: d }));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Workflow list' }));
    expect(where()).toBe('/p/t/workflow/list');
    await user.click(screen.getByRole('button', { name: 'Back to the board' }));
    await screen.findByRole('region', { name: 'Design board' });
    await user.keyboard('{Control>}k{/Control}');
    await user.keyboard('workflow list{Enter}');
    expect(await screen.findByRole('button', { name: 'Back to the board' })).toBeInTheDocument();
  });
});
