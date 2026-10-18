import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BoardStyleSwitch } from '../pages/BoardPage';
import type { SeatView } from '../generated/SeatView';
import { compiledRig, seatView } from '../test-fixtures';
import { BOARD_STYLE_KEY, logoRouteBox, readBoardStyle, setBoardStyle } from './boardStyle';
import { buildCommands, type CommandContext } from './commands';
import { forgetViewports, LiveGraph } from './LiveGraph';
import { teamAgents, type AgentRow } from './model';

function rows(s: Record<string, SeatView>): Record<string, AgentRow> {
  return Object.fromEntries(teamAgents('r', compiledRig(), s, {}, {}).map((a) => [a.id, a]));
}
const working = { 'dev.a@r': seatView('a', { activity: 'working' }), 'dev.b@r': seatView('b') };

const slots = () => [...document.querySelectorAll<HTMLElement>('.card-slot')].map((el) => [el.style.left, el.style.top, el.style.width, el.style.height].join());
const node = (name: RegExp) => screen.getByRole('button', { name }).closest('.agent-box') as HTMLElement;

/** jsdom has no matchMedia: answer "reduce" when asked to. */
function motionPreference(reduce: boolean) {
  window.matchMedia = vi.fn().mockImplementation((q: string) => ({
    matches: reduce && q.includes('reduce'), media: q, onchange: null,
    addEventListener: () => {}, removeEventListener: () => {}, addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

beforeEach(() => { localStorage.clear(); forgetViewports(); motionPreference(false); });
afterEach(() => { // @ts-expect-error jsdom has none of its own
  delete window.matchMedia; });

describe('Board style: Cards | Logos', () => {
  it('is remembered per browser, defaults to Cards, and survives storage that throws', () => {
    expect(readBoardStyle()).toBe('cards');
    act(() => setBoardStyle('logos'));
    expect(localStorage.getItem(BOARD_STYLE_KEY)).toBe('logos');
    expect(readBoardStyle()).toBe('logos');
    localStorage.setItem(BOARD_STYLE_KEY, 'sparkly');
    expect(readBoardStyle()).toBe('cards');
    const get = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    const set = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    expect(readBoardStyle()).toBe('cards');
    expect(() => setBoardStyle('logos')).not.toThrow();
    get.mockRestore();
    set.mockRestore();
  });

  it('the toolbar switch swaps cards for logos without moving any agent, and back', async () => {
    const user = userEvent.setup();
    render(<><BoardStyleSwitch /><LiveGraph rig="r" compiled={compiledRig()} agents={rows(working)} onOpen={() => {}} onAnswer={() => {}} now={0} /></>);
    const before = slots();
    expect(document.querySelectorAll('.is-logo')).toHaveLength(0);
    const sw = screen.getByRole('group', { name: 'Board style' });
    expect(screen.getByRole('button', { name: 'Cards' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(screen.getByRole('button', { name: 'Logos' }));
    expect(sw).toBeInTheDocument();
    expect(localStorage.getItem(BOARD_STYLE_KEY)).toBe('logos');
    expect(document.querySelectorAll('.agent-box.is-logo')).toHaveLength(2);
    expect(document.querySelector('.box-foot')).toBeNull();
    expect(slots()).toEqual(before);
    // Arrows end at the logo, inside the same slot; their labels stay.
    expect(screen.getByTestId('edge-label-dev.a@r-dev.b@r')).toHaveTextContent('hands work to');
    await user.click(screen.getByRole('button', { name: 'Cards' }));
    expect(document.querySelectorAll('.is-logo')).toHaveLength(0);
    expect(slots()).toEqual(before);
  });

  it('a working agent animates, an idle one stays still, and reduced motion keeps only a still ring', () => {
    setBoardStyle('logos');
    const { unmount } = render(<LiveGraph rig="r" compiled={compiledRig()} agents={rows(working)} onOpen={() => {}} onAnswer={() => {}} now={0} />);
    const a = node(/^a — working/), b = node(/^b — idle/);
    expect(a).toHaveClass('is-logo', 'logo-working', 'logo-animated', 'v-practice');
    expect(b).toHaveClass('is-logo');
    expect(b).not.toHaveClass('logo-working');
    expect(b).not.toHaveClass('logo-animated');
    // Status is a shape and words, not the logo's motion alone.
    expect(a).toHaveTextContent('working');
    unmount();
    motionPreference(true);
    render(<LiveGraph rig="r" compiled={compiledRig()} agents={rows(working)} onOpen={() => {}} onAnswer={() => {}} now={0} />);
    expect(node(/^a — working/)).toHaveClass('logo-working');
    expect(node(/^a — working/)).not.toHaveClass('logo-animated');
  });

  it('stops moving while the tab is hidden', () => {
    setBoardStyle('logos');
    render(<LiveGraph rig="r" compiled={compiledRig()} agents={rows(working)} onOpen={() => {}} onAnswer={() => {}} now={0} />);
    expect(node(/^a — working/)).toHaveClass('logo-animated');
    const vis = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden');
    act(() => { document.dispatchEvent(new Event('visibilitychange')); });
    expect(node(/^a — working/)).not.toHaveClass('logo-animated');
    vis.mockReturnValue('visible');
    act(() => { document.dispatchEvent(new Event('visibilitychange')); });
    expect(node(/^a — working/)).toHaveClass('logo-animated');
    vis.mockRestore();
  });

  it('needs you shows a badge and the words; click and Enter still open the agent', async () => {
    setBoardStyle('logos');
    const onOpen = vi.fn();
    const seats = { 'dev.a@r': seatView('a', { activity: 'waiting-input' }), 'dev.b@r': seatView('b') };
    render(<LiveGraph rig="r" compiled={compiledRig()} agents={rows(seats)} onOpen={onOpen} onAnswer={() => {}} now={0} />);
    const a = screen.getByRole('button', { name: /^a — needs you/ });
    expect(a.closest('.agent-box')!.querySelector('.logo-badge')).not.toBeNull();
    expect(a).toHaveTextContent('needs you');
    const user = userEvent.setup();
    await user.click(a);
    expect(onOpen).toHaveBeenLastCalledWith('dev.a@r');
    screen.getByRole('button', { name: /^b — idle/ }).focus();
    await user.keyboard('{Enter}');
    expect(onOpen).toHaveBeenLastCalledWith('dev.b@r');
  });

  it("routes arrows to a centred column inside the card's slot", () => {
    expect(logoRouteBox({ x: 100, y: 40, w: 200, h: 200 })).toEqual({ x: 152, y: 64, w: 96, h: 152 });
  });

  it('Ctrl K has Board style: Logos and Board style: Cards on both boards; the one in use says so', () => {
    const noop = () => {};
    const ctx = (view: CommandContext['view']): CommandContext => ({ view, picked: 0, runningAgents: 0, projects: [], go: noop, act: new Proxy({}, { get: () => noop }) as CommandContext['act'] });
    for (const view of ['structure', 'team'] as const) {
      const cmds = buildCommands(ctx(view));
      expect(cmds.find((c) => c.id === 'board.style.logos')).toMatchObject({ title: 'Board style: Logos', disabled: null });
      expect(cmds.find((c) => c.id === 'board.style.cards')).toMatchObject({ title: 'Board style: Cards', disabled: 'In use' });
    }
    expect(buildCommands(ctx('home')).find((c) => c.id === 'board.style.logos')).toBeUndefined();
    act(() => buildCommands(ctx('team')).find((c) => c.id === 'board.style.logos')!.run());
    expect(readBoardStyle()).toBe('logos');
    expect(buildCommands(ctx('team')).find((c) => c.id === 'board.style.logos')!.disabled).toBe('In use');
  });
});
