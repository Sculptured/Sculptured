import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { TeamDraft } from '../generated/TeamDraft';
import { BOARD_STYLE_KEY } from '../shell/boardStyle';
import { forgetViewports } from '../shell/viewport';
import { teamDraft, teamRecord } from '../test-fixtures';
import { saveTiming } from '../useTeamDraft';
import { WorkflowGraph } from '../workflow/WorkflowGraph';
import { card, fakeTeam, pos, renderAt, resetApi, where } from './harness';
import { addInputLink, addInputNote, countsText, inputCounts, inputItems, normalizeLink, removeInputItem } from './inputItems';
import { InputMaterial, LINK_HELP } from './InputMaterial';

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

async function board() {
  fakeTeam(teamRecord());
  renderAt('/team/t/structure');
  await screen.findByRole('region', { name: 'Design board' });
}

describe('Design board in Logos style', () => {
  it('swaps cards for logos at the same places; click, Enter and drag still work', async () => {
    await board();
    const user = userEvent.setup();
    const cardsAt = [pos('Builder'), pos('Reviewer')];
    await user.click(within(screen.getByRole('group', { name: 'Board style' })).getByRole('button', { name: 'Logos' }));
    expect(localStorage.getItem(BOARD_STYLE_KEY)).toBe('logos');
    expect(document.querySelectorAll('.sb-card.is-logo')).toHaveLength(2);
    expect([pos('Builder'), pos('Reviewer')]).toEqual(cardsAt);
    expect(card('Builder')).toHaveTextContent('Practice · free');
    // Full text belongs to cards: it says so instead of doing nothing.
    await user.click(screen.getByRole('button', { name: 'More tools' }));
    expect(screen.getByRole('button', { name: 'Full text' })).toBeDisabled();
    // A drag still moves the agent (and is not a click).
    const builder = card('Builder');
    fireEvent.pointerDown(builder, { button: 0, clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 180, clientY: 260, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 180, clientY: 260, pointerId: 1 });
    fireEvent.click(builder);
    expect(pos('Builder')).toEqual([120, 200]);
    expect(where()).toBe('/p/t/board?mode=design');
    fireEvent.keyDown(card('Builder'), { key: 'Enter' });
    expect(await screen.findByRole('region', { name: 'Setup' })).toBeVisible();
    expect(where()).toBe('/p/t/agents/team.builder');
  });

  it('a click on a logo opens its agent', async () => {
    localStorage.setItem(BOARD_STYLE_KEY, 'logos');
    await board();
    expect(card('Reviewer').closest('.sb-card')).toHaveClass('is-logo');
    await userEvent.setup().click(card('Reviewer'));
    expect(where()).toBe('/p/t/agents/team.reviewer');
  });
});

describe('The Input: what was added', () => {
  it('reads links, notes and files from the text and changes one without touching the rest', () => {
    const text = 'https://example.com/course\nGoal: teach series.\nShow proofs.\n\n- ask about Gibbs\n/home/me/notes.pdf';
    const items = inputItems(text);
    expect(items.map((i) => i.kind)).toEqual(['link', 'note', 'note', 'file']);
    expect(items[1].text).toBe('Goal: teach series.\nShow proofs.');
    expect(countsText(inputCounts(items))).toBe('1 link · 2 notes · 1 file');
    expect(removeInputItem(text, items[1])).toBe('https://example.com/course\n\n- ask about Gibbs\n/home/me/notes.pdf');
    expect(addInputLink('', 'https://a.test')).toBe('https://a.test');
    expect(addInputNote('https://a.test', ' Idea ')).toBe('https://a.test\n\nIdea');
    expect(normalizeLink('example.com/x')).toBe('https://example.com/x');
    expect(normalizeLink('not a link')).toBeNull();
    expect(countsText(inputCounts(inputItems('')))).toBe('');
  });

  it('the board card invites when empty and previews counts and items when filled', () => {
    const draft = (input: string): TeamDraft => teamDraft({ connections: [], workflow: { input, dispatcher: 'builder', collector: 'reviewer', entry: 'b', max_tasks: 4, max_steps: 4, stages: [{ id: 'b', name: 'Build', agent: 'builder', routes: { done: null } }] } });
    const { rerender } = render(<WorkflowGraph draft={draft('')} onOpen={() => {}} onReveal={() => {}} />);
    const node = screen.getByRole('button', { name: 'Input: links, notes and ideas' });
    expect(node).toHaveClass('input-card', 'is-empty');
    expect(node).toHaveTextContent('Nothing yet. Add links, notes or files');
    expect(node).toHaveTextContent('Open to add');
    rerender(<WorkflowGraph draft={draft('https://example.com/course\n\nGoal one\n\nGoal two\n\nGoal three')} onOpen={() => {}} onReveal={() => {}} />);
    expect(node).not.toHaveClass('is-empty');
    expect(node).toHaveTextContent('1 link · 3 notes');
    expect(node).toHaveTextContent('example.com/course');
    expect(node).toHaveTextContent('+2 more');
    expect(node).toHaveTextContent('Open to edit');
    rerender(<WorkflowGraph draft={draft('x')} onOpen={() => {}} onReveal={() => {}} readOnly />);
    expect(node).toHaveTextContent('Open to read');
  });

  it('the editor adds a link and a note, explains a bad link, lists items and removes one', async () => {
    function Editor() {
      const [text, setText] = useState('');
      return <><InputMaterial text={text} onChange={setText} locked={false} lockedReason="" rawLabel="Workflow input" folder={null} /><output data-testid="text">{text}</output></>;
    }
    render(<Editor />);
    const user = userEvent.setup();
    expect(screen.getByText(/Nothing added yet/)).toBeInTheDocument();
    await user.type(screen.getByLabelText('Add a link'), 'not a link');
    await user.click(screen.getByRole('button', { name: 'Add link' }));
    expect(screen.getByRole('alert')).toHaveTextContent(LINK_HELP);
    await user.clear(screen.getByLabelText('Add a link'));
    await user.type(screen.getByLabelText('Add a link'), 'example.com/notes{Enter}');
    await user.type(screen.getByLabelText('Add a note'), 'Teach why it converges');
    await user.click(screen.getByRole('button', { name: 'Add note' }));
    expect(screen.getByTestId('text')).toHaveTextContent('https://example.com/notes Teach why it converges');
    expect(screen.getByRole('heading', { level: 2 })).toHaveTextContent('Added · 1 link · 1 note');
    expect(screen.getByRole('link', { name: 'example.com/notes' })).toHaveAttribute('href', 'https://example.com/notes');
    await user.click(screen.getByRole('button', { name: 'Remove link: example.com/notes' }));
    expect(screen.getByTestId('text').textContent).toBe('Teach why it converges');
    expect(screen.getByLabelText('Workflow input')).toHaveValue('Teach why it converges');
  });

  it('while locked says why and changes nothing', () => {
    render(<InputMaterial text="https://example.com" onChange={() => {}} locked lockedReason="Stop the project before changing Input." rawLabel="Workflow input" folder={null} />);
    expect(screen.getByRole('status')).toHaveTextContent('Stop the project before changing Input.');
    expect(screen.getByLabelText('Add a link')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Remove link: example.com' })).toBeDisabled();
    expect(screen.getByLabelText('Workflow input')).toHaveAttribute('readonly');
  });
});

it('the project Input box shows what it holds and who gets it', async () => {
  fakeTeam(teamRecord({ draft: teamDraft({ input: { text: 'https://example.com/spec\n\nShip the landing page', x: -300, y: 40, to: ['builder'] } }) }));
  renderAt('/team/t/structure');
  const node = await screen.findByRole('button', { name: /^Input: / });
  await waitFor(() => expect(node).toHaveTextContent('1 link · 1 note'));
  expect(node).toHaveTextContent('→ Builder');
  expect(node).toHaveTextContent('Edit');
  expect(node.closest('.team-input-slot')!.querySelector('.port.out')).not.toBeNull();
});
