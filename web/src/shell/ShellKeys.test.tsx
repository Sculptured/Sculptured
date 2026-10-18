import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import { resetSeenCache } from '../seen';
import { ev, FakeEventSource, seatView, snapshot } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { forgetViewports } from './LiveGraph';
import { snapQ, stream, twoTeams, viewQ, where } from './shellHarness';
import { TOAST_MS } from './Toasts';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn(), teamRecent: vi.fn(), listRigs: vi.fn(), interrupt: vi.fn(), send: vi.fn() } };
});

beforeEach(() => {
  FakeEventSource.reset();
  forgetViewports();
  localStorage.clear();
  resetSeenCache();
  vi.mocked(api.latestSeatEvents).mockReset().mockResolvedValue([]);
  vi.mocked(api.teamRecent).mockReset().mockResolvedValue({});
  vi.mocked(api.listRigs).mockReset().mockResolvedValue([]);
  vi.mocked(api.interrupt).mockReset().mockResolvedValue({});
  vi.mocked(api.send).mockReset().mockResolvedValue({ message_id: 'm1' });
});
afterEach(() => vi.useRealTimers());

describe('the Run board and an agent', () => {
  it('a click opens the agent; Esc returns to the same board with its viewport and selection kept', async () => {
    await twoTeams();
    const user = userEvent.setup();
    await user.keyboard('+');
    const zoomed = screen.getByTestId('graph-world').style.transform;
    await user.click(screen.getByRole('button', { name: 'b — idle' }));
    expect(where()).toBe('/p/r/agents/dev.b');
    expect(screen.queryByTestId('graph-world')).not.toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(where()).toBe('/p/r/board');
    expect(screen.getByTestId('graph-world').style.transform).toBe(zoomed);
    const b = screen.getByRole('button', { name: 'b — idle' });
    expect(b).toHaveAttribute('aria-pressed', 'true');
    expect(b).toHaveFocus();
  });

  it('Esc in the message box leaves the box first, keeping the draft', async () => {
    await twoTeams('/rig/r/agent/dev.a%40r');
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Message'), 'draft');
    await user.keyboard('{Escape}');
    expect(where()).toBe('/p/r/agents/dev.a');
    expect(screen.getByLabelText('Message')).toHaveValue('draft');
    await user.keyboard('{Escape}');
    expect(where()).toBe('/p/r/board');
  });

  it('i interrupts the current turn; Enter sends, Shift+Enter adds a line, Ctrl+C stays copy', async () => {
    await twoTeams('/rig/r/agent/dev.a%40r', snapshot({ seats: [seatView('a', { activity: 'working', current_turn: 't1' }), seatView('b')] }));
    const user = userEvent.setup();
    await user.keyboard('i');
    expect(api.interrupt).toHaveBeenCalledWith('dev.a@r', expect.any(String));
    vi.mocked(api.interrupt).mockClear();
    const box = screen.getByLabelText('Message');
    await user.type(box, 'line one{Shift>}{Enter}{/Shift}line two');
    expect(box).toHaveValue('line one\nline two');
    await user.keyboard('{Enter}');
    expect(api.send).toHaveBeenCalledWith('dev.a@r', { text: 'line one\nline two', intent: 'inform', interrupt: false }, expect.any(String));
    await waitFor(() => expect(box).toHaveValue(''));
    await user.keyboard('{Control>}c{/Control}');
    expect(api.interrupt).not.toHaveBeenCalled();
  });

  it('Answer on a "needs you" card opens the agent with the message box focused', async () => {
    await twoTeams('/rig/r', snapshot({ seats: [seatView('a', { activity: 'waiting-input' }), seatView('b')] }));
    await userEvent.setup().click(screen.getByRole('button', { name: 'Answer a' }));
    expect(where()).toBe('/p/r/agents/dev.a');
    expect(screen.getByLabelText('Message')).toHaveFocus();
  });
});

describe('done state', () => {
  const turnEnd = (seq: number) => ({ event: ev(seq, { type: 'turn-ended', turn_id: `t${seq}`, status: 'completed' }), seat: seatView('a') });

  it('marks an agent done when a turn ends, never verified, and clears it once you view the agent', async () => {
    await twoTeams('/rig/r', snapshot({ seats: [seatView('a', { activity: 'working', current_turn: 't11' }), seatView('b')] }));
    act(() => stream().emit('event', turnEnd(11)));
    const card = screen.getByRole('button', { name: 'a — done (reported)' });
    expect(card).toHaveTextContent('✓done (reported)');
    expect(card.querySelector('.st-done')).toHaveAttribute('data-tip', 'turn finished; not verified');
    const user = userEvent.setup();
    await user.click(card);
    await user.keyboard('{Escape}');
    expect(screen.getByRole('button', { name: 'a — idle' })).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem('sculptured.seen.v1')!)).toEqual({ 'dev.a@r': 11 });
  });

  it('comes back after a later turn ends', async () => {
    localStorage.setItem('sculptured.seen.v1', JSON.stringify({ 'dev.a@r': 11 }));
    resetSeenCache();
    vi.mocked(api.teamRecent).mockImplementation(async (rig): Promise<Record<string, EventRecord[]>> =>
      rig === 'r' ? { 'dev.a@r': [ev(11, { type: 'turn-ended', turn_id: 't11', status: 'completed' })] } : {},
    );
    await twoTeams();
    await waitFor(() => expect(api.teamRecent).toHaveBeenCalledWith('r', 60));
    expect(await screen.findByRole('button', { name: 'a — idle' })).toBeInTheDocument();
    act(() => stream().emit('event', turnEnd(14)));
    expect(screen.getByRole('button', { name: 'a — done (reported)' })).toBeInTheDocument();
  });
});

describe('Go to and the command palette', () => {
  it('g searches every project’s agents by name, team and folder; Enter opens one', async () => {
    await twoTeams();
    const user = userEvent.setup();
    await user.keyboard('g');
    const dialog = screen.getByRole('dialog', { name: 'Go to agent' });
    const box = within(dialog).getByRole('combobox');
    expect(box).toHaveFocus();
    expect(within(dialog).getAllByRole('option')).toHaveLength(4);
    await user.type(box, 'y-folder');
    expect(within(dialog).getAllByRole('option').map((o) => o.getAttribute('aria-label'))).toEqual(['y — idle']);
    await user.keyboard('{Enter}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(where()).toBe('/p/q/agents/ops.y');
  });

  it('Ctrl+K finds agents too, and Esc closes it where you were', async () => {
    await twoTeams();
    const user = userEvent.setup();
    await user.keyboard('{Control>}k{/Control}');
    const dialog = screen.getByRole('dialog', { name: 'Command palette' });
    await user.keyboard('y-folder');
    expect(within(dialog).getAllByRole('option').map((o) => o.getAttribute('aria-label'))).toContain('y — idle');
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(where()).toBe('/p/r/board');
  });

  it('Alt+b, Alt+w, Alt+i and Alt+d filter by state; bare letters type', async () => {
    await twoTeams('/rig/r', snapshot({ seats: [seatView('a', { activity: 'working' }), seatView('b', { lifecycle: 'attention', lifecycle_reason: 'x' })] }));
    const user = userEvent.setup();
    await user.keyboard('g');
    const dialog = screen.getByRole('dialog', { name: 'Go to agent' });
    const names = () => within(dialog).queryAllByRole('option').map((o) => o.getAttribute('aria-label'));
    await user.keyboard('{Alt>}b{/Alt}');
    expect(names()).toEqual(['b — needs you']);
    await user.keyboard('{Alt>}b{/Alt}{Alt>}w{/Alt}');
    expect(names()).toEqual(['a — working']);
    await user.keyboard('{Alt>}w{/Alt}{Alt>}i{/Alt}');
    expect(names()).toEqual(['x — idle', 'y — idle']);
    await user.keyboard('{Alt>}i{/Alt}bw');
    expect(within(dialog).getByRole('combobox')).toHaveValue('bw');
  });
});

describe('notifications', () => {
  it('toasts a background agent that needs you until clicked', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await twoTeams();
    act(() =>
      stream().emit('event', {
        event: { ...ev(6, { type: 'seat-lifecycle-changed', lifecycle: 'attention', reason: 'stuck' }), subject: { kind: 'seat', id: 'ops.y@q' } },
        seat: viewQ('y', { lifecycle: 'attention', lifecycle_reason: 'stuck' }),
      }),
    );
    const toasts = screen.getByRole('region', { name: 'Notifications' });
    expect(toasts).toHaveTextContent('◉ y needs you · q');
    act(() => vi.advanceTimersByTime(TOAST_MS * 3));
    expect(toasts).toHaveTextContent('y needs you');
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    await user.click(within(toasts).getByRole('button', { name: 'y needs you · q' }));
    expect(where()).toBe('/p/q/agents/ops.y');
    expect(toasts).not.toHaveTextContent('y needs you');
  });

  it('toasts "done" when a background turn ends, and dismisses it after 6 s', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await twoTeams('/rig/r', snapshot(), snapQ({ seats: [viewQ('x', { activity: 'working', current_turn: 't' }), viewQ('y')] }));
    act(() =>
      stream().emit('event', {
        event: { ...ev(6, { type: 'turn-ended', turn_id: 't', status: 'completed' }), subject: { kind: 'seat', id: 'ops.x@q' } },
        seat: viewQ('x'),
      }),
    );
    const toasts = screen.getByRole('region', { name: 'Notifications' });
    expect(toasts).toHaveTextContent('✓ x done (reported) · q');
    act(() => vi.advanceTimersByTime(TOAST_MS + 100));
    expect(toasts).not.toHaveTextContent('x done');
  });

  it('does not toast the agent you are looking at, or what was already true on load', async () => {
    await twoTeams('/rig/r/agent/dev.a%40r', snapshot({ seats: [seatView('a', { activity: 'working' }), seatView('b', { lifecycle: 'attention', lifecycle_reason: 'x' })] }));
    act(() => stream().emit('event', { event: ev(11, { type: 'turn-ended', turn_id: 't', status: 'completed' }), seat: seatView('a') }));
    expect(within(screen.getByRole('region', { name: 'Notifications' })).queryAllByRole('listitem')).toHaveLength(0);
  });
});

describe('keys', () => {
  it('? opens the full keymap, filterable, in plain sections', async () => {
    await twoTeams();
    const user = userEvent.setup();
    await user.keyboard('?');
    const help = screen.getByRole('dialog', { name: 'Keys' });
    expect(help).toHaveTextContent('interrupt the current turn');
    expect(within(help).getByRole('heading', { name: 'Board · Design' })).toBeInTheDocument();
    expect(help).toHaveTextContent('filter: needs you or failed · working · idle · turn finished (✓)');
    await user.type(within(help).getByRole('textbox', { name: 'Filter keys' }), 'zoom');
    expect(help).not.toHaveTextContent('interrupt the current turn');
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('bare letters do nothing while typing', async () => {
    await twoTeams('/rig/r/agent/dev.a%40r');
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Message'), 'g?l');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Message')).toHaveValue('g?l');
  });

  it('l opens the Agents list from the board; no OpenRig words with the board, palette, help and list open', async () => {
    const q = snapQ({ seats: [viewQ('x', { lifecycle: 'attention', lifecycle_reason: "host lost this seat's process" }), viewQ('y')] });
    await twoTeams('/rig/r', snapshot(), q);
    const user = userEvent.setup();
    const found = () => openRigWords(document.querySelector<HTMLElement>('.app')!);
    expect(found()).toEqual([]);
    await user.keyboard('g');
    expect(found()).toEqual([]);
    await user.keyboard('{Escape}?');
    expect(found()).toEqual([]);
    await user.keyboard('{Escape}l');
    expect(where()).toBe('/p/r/agents');
    expect(found()).toEqual([]);
  });
});
