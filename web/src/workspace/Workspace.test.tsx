import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { emptyStack, panes, stacks, type Node, type PaneRef, type Split, type Stack } from './model';
import { createRegistry, type PaneProps } from './registry';
import { Workspace } from './Workspace';

const Body = ({ instance }: PaneProps) => <div data-testid={`body-${instance}`}>Body of {instance}</div>;
const Form = ({ settings, setSettings }: PaneProps) => (
  <label>
    Lines
    <input value={String(settings.lines)} onChange={(e) => setSettings({ ...settings, lines: Number(e.target.value) })} />
  </label>
);
const registry = createRegistry([
  { type: 'logs', title: 'Logs', glyph: '≡', multi: true, defaultSettings: { lines: 10 }, Settings: Form, Body },
  { type: 'agents', title: 'Agents', glyph: '◉', Body },
  { type: 'world', title: 'World', glyph: '◇', Body },
]);

const TYPE: Record<string, string> = { a: 'logs', b: 'agents', c: 'world' };
const st = (id: string, ids: string[], active = 0): Stack => ({ kind: 'stack', id, panes: ids.map((p): PaneRef => ({ id: p, type: TYPE[p] })), active });
const row = (id: string, ...kids: Node[]): Split => ({ kind: 'split', id, dir: 'row', kids, sizes: kids.map(() => 1 / kids.length) });

function setup(initial: Node, extra: { onPopOut?: (id: string) => void } = {}) {
  const onChange = vi.fn<(root: Node) => void>();
  function Harness({ ctx }: { ctx?: unknown }) {
    const [root, setRoot] = useState(initial);
    const change = (next: Node) => { onChange(next); setRoot(next); };
    return <Workspace root={root} registry={registry} ctx={ctx} onChange={change} onPopOut={extra.onPopOut} />;
  }
  const view = render(<Harness ctx={{ tick: 0 }} />);
  const last = () => onChange.mock.calls[onChange.mock.calls.length - 1]?.[0];
  return { ...view, onChange, last, rerenderCtx: (ctx: unknown) => view.rerender(<Harness ctx={ctx} />) };
}

/** Gives each stack element a fixed box, since jsdom has no layout. */
function boxes(map: Record<string, [number, number, number, number]>) {
  for (const [id, [x, y, w, h]] of Object.entries(map)) {
    const el = document.querySelector<HTMLElement>(`[data-stack="${id}"]`)!;
    el.getBoundingClientRect = () => ({ x, y, left: x, top: y, width: w, height: h, right: x + w, bottom: y + h, toJSON: () => ({}) });
  }
}

const tab = (title: string) => screen.getByRole('tab', { name: new RegExp(title) });
const overlay = () => document.querySelector<HTMLElement>('[data-drop-zone]');
const grab = (el: HTMLElement, x: number, y: number) => fireEvent.pointerDown(el, { button: 0, clientX: x, clientY: y, pointerId: 1 });
const move = (x: number, y: number) => fireEvent.pointerMove(window, { clientX: x, clientY: y, pointerId: 1 });
const release = (x: number, y: number) => fireEvent.pointerUp(window, { clientX: x, clientY: y, pointerId: 1 });
const shape = (root: Node) => stacks(root).map((s) => s.panes.map((p) => p.id));

describe('Workspace', () => {
  it('drag_tab_to_edge_splits', () => {
    const { last } = setup(st('s1', ['a', 'b']));
    boxes({ s1: [0, 0, 400, 300] });
    grab(tab('Agents'), 60, 10);
    move(62, 11);
    expect(overlay()).toBeNull();
    move(390, 150);
    expect(overlay()?.dataset.dropZone).toBe('right');
    expect(overlay()!.closest('[data-stack]')).toHaveAttribute('data-stack', 's1');
    move(200, 150);
    expect(overlay(), 'joining its own tabs changes nothing').toBeNull();
    move(200, 290);
    expect(overlay()?.dataset.dropZone).toBe('bottom');
    move(390, 150);
    release(390, 150);
    const root = last() as Split;
    expect(root.kind).toBe('split');
    expect(root.dir).toBe('row');
    expect(shape(root)).toEqual([['a'], ['b']]);
    expect(overlay()).toBeNull();
  });

  it('escape_cancels_drag', () => {
    const { onChange } = setup(row('r', st('s1', ['a']), st('s2', ['b'])));
    boxes({ s1: [0, 0, 400, 300], s2: [400, 0, 400, 300] });
    grab(tab('Logs'), 20, 10);
    move(410, 150);
    expect(overlay()?.dataset.dropZone).toBe('left');
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(overlay()).toBeNull();
    move(420, 150);
    expect(overlay()).toBeNull();
    release(420, 150);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('drop that changes nothing shows no target and does nothing', () => {
    const { onChange } = setup(row('r', st('s1', ['a']), st('s2', ['b'])));
    boxes({ s1: [0, 0, 400, 300], s2: [400, 0, 400, 300] });
    grab(tab('Logs'), 20, 10);
    move(20, 280);
    expect(overlay()).toBeNull();
    release(20, 280);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('menu_move_to_as_tab_and_left', async () => {
    const user = userEvent.setup();
    const { last } = setup(row('r', st('s1', ['a']), st('s2', ['b']), st('s3', ['c'])));
    screen.getByRole('button', { name: 'Logs options' }).focus();
    await user.keyboard('{Enter}');
    expect(screen.getByRole('menuitem', { name: /Move to/ })).toHaveFocus();
    await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('menuitem', { name: 'Left of Agents' })).toHaveFocus();
    await user.keyboard('{ArrowDown}{ArrowDown}{ArrowDown}{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: 'As a tab with Agents' })).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(shape(last()!)).toEqual([['b', 'a'], ['c']]);
    expect(screen.queryByRole('menu')).toBeNull();
    expect(tab('Logs')).toHaveFocus();

    screen.getByRole('button', { name: 'Logs options' }).focus();
    await user.keyboard('{Enter}{ArrowRight}');
    expect(screen.getByRole('menuitem', { name: 'Left of World' })).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(shape(last()!)).toEqual([['b'], ['a'], ['c']]);
  });

  it('menu keys: back, escape returns focus, and nothing to move beside says why', async () => {
    const user = userEvent.setup();
    setup(st('s1', ['a']));
    const opener = screen.getByRole('button', { name: 'Logs options' });
    opener.focus();
    await user.keyboard('{Enter}');
    const moveTo = screen.getByRole('menuitem', { name: /Move to/ });
    expect(moveTo).toBeDisabled();
    expect(moveTo).toHaveTextContent('nothing to move beside');
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(opener).toHaveFocus();
  });

  it('splitter_keyboard_resizes', async () => {
    const user = userEvent.setup();
    const { last, onChange } = setup(row('r', st('s1', ['a']), st('s2', ['b'])));
    const sep = screen.getByRole('separator');
    expect(sep).toHaveAttribute('aria-orientation', 'vertical');
    expect(sep).toHaveAttribute('tabindex', '0');
    sep.focus();
    await user.keyboard('{ArrowRight}');
    expect((last() as Split).sizes[0]).toBeCloseTo(0.55);
    await user.keyboard('{ArrowLeft}{ArrowLeft}');
    expect((last() as Split).sizes[0]).toBeCloseTo(0.45);
    const calls = onChange.mock.calls.length;
    await user.keyboard('{ArrowUp}');
    expect(onChange).toHaveBeenCalledTimes(calls);

    sep.parentElement!.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, width: 1000, height: 500, right: 1000, bottom: 500, toJSON: () => ({}) });
    grab(sep, 450, 100);
    move(500, 100);
    move(600, 100);
    release(600, 100);
    expect((last() as Split).sizes[0]).toBeCloseTo(0.6);
  });

  it('close_and_settings_and_duplicate', async () => {
    const user = userEvent.setup();
    const onPopOut = vi.fn();
    const { last } = setup(st('s1', ['a', 'b'], 1), { onPopOut });
    await user.click(screen.getByRole('button', { name: 'Agents options' }));
    const menu = screen.getByRole('menu');
    expect(within(menu).queryByRole('menuitem', { name: 'Duplicate' })).toBeNull();
    expect(within(menu).queryByRole('menuitem', { name: 'Settings' })).toBeNull();
    await user.click(within(menu).getByRole('menuitem', { name: 'Pop out' }));
    expect(onPopOut).toHaveBeenCalledWith('b');

    await user.click(screen.getByRole('button', { name: 'Close Agents' }));
    expect(shape(last()!)).toEqual([['a']]);

    await user.click(screen.getByRole('button', { name: 'Logs options' }));
    await user.click(screen.getByRole('menuitem', { name: 'Settings' }));
    const field = screen.getByRole('textbox', { name: 'Lines' });
    expect(field).toHaveValue('10');
    await user.clear(field);
    await user.type(field, '25');
    expect(panes(last()!)[0].settings).toEqual({ lines: 25 });
    await user.click(screen.getByRole('button', { name: 'Done' }));
    expect(screen.queryByRole('textbox', { name: 'Lines' })).toBeNull();

    await user.click(screen.getByRole('button', { name: 'Logs options' }));
    await user.click(screen.getByRole('menuitem', { name: 'Duplicate' }));
    const twins = panes(last()!);
    expect(twins).toHaveLength(2);
    expect(twins[1].id).not.toBe('a');
    expect(twins[1].settings).toEqual({ lines: 25 });

    await user.click(screen.getAllByRole('button', { name: 'Logs options' })[0]);
    await user.click(screen.getByRole('menuitem', { name: 'Close' }));
    expect(panes(last()!)).toHaveLength(1);
  });

  it('stream_update_keeps_drag_and_scroll', () => {
    const { last, rerenderCtx } = setup(st('s1', ['a', 'b']));
    boxes({ s1: [0, 0, 400, 300] });
    const body = screen.getByTestId('body-a');
    const scroller = body.closest<HTMLElement>('[role="tabpanel"]')!;
    scroller.scrollTop = 40;
    fireEvent.scroll(scroller);
    grab(tab('Agents'), 60, 10);
    move(390, 150);
    rerenderCtx({ tick: 1 });
    act(() => void rerenderCtx({ tick: 2 }));
    expect(overlay()?.dataset.dropZone).toBe('right');
    expect(screen.getByTestId('body-a')).toBe(body);
    expect(scroller.scrollTop).toBe(40);
    move(392, 150);
    release(392, 150);
    expect(shape(last()!)).toEqual([['a'], ['b']]);
    expect(screen.getByTestId('body-a')).toBe(body);
    expect(screen.getByTestId('body-a').closest('[role="tabpanel"]')).toBe(scroller);
    expect(scroller.scrollTop).toBe(40);
  });

  it('drag_into_centre_then_left_and_top', () => {
    const { last } = setup(row('r', st('s1', ['a']), st('s2', ['b', 'c'])));
    boxes({ s1: [0, 0, 400, 300], s2: [400, 0, 400, 300] });
    grab(tab('Logs'), 20, 10);
    move(600, 150);
    expect(overlay()?.dataset.dropZone).toBe('center');
    release(600, 150);
    expect(shape(last()!)).toEqual([['b', 'c', 'a']]);

    boxes({ s2: [0, 0, 800, 300] });
    grab(tab('World'), 100, 10);
    move(10, 150);
    expect(overlay()?.dataset.dropZone).toBe('left');
    release(10, 150);
    expect(shape(last()!)).toEqual([['c'], ['b', 'a']]);

    const [left, right] = stacks(last()!).map((s) => s.id);
    boxes({ [left]: [0, 0, 400, 300], [right]: [400, 0, 400, 300] });
    grab(tab('Agents'), 420, 10);
    move(200, 10);
    expect(overlay()?.dataset.dropZone).toBe('top');
    release(200, 10);
    const root = last() as Split;
    expect(shape(root)).toEqual([['b'], ['c'], ['a']]);
    expect(root.kids[0]).toMatchObject({ kind: 'split', dir: 'col' });
  });

  it('focus_stays_in_stack_after_close', async () => {
    const user = userEvent.setup();
    setup(row('r', st('s1', ['a', 'b']), st('s2', ['c'])), { onPopOut: vi.fn() });
    const logsMenu = screen.getByRole('button', { name: 'Logs options' });
    await user.click(logsMenu);
    await user.click(screen.getByRole('menuitem', { name: 'Settings' }));
    await user.click(screen.getByRole('button', { name: 'Done' }));
    expect(logsMenu).toHaveFocus();
    await user.click(logsMenu);
    await user.click(screen.getByRole('menuitem', { name: 'Pop out' }));
    expect(logsMenu).toHaveFocus();

    await user.click(screen.getByRole('button', { name: 'Close Logs' }));
    expect(tab('Agents')).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'Agents options' }));
    await user.click(screen.getByRole('menuitem', { name: 'Close' }));
    expect(tab('World')).toHaveFocus();
    expect(document.activeElement).not.toBe(document.body);
  });

  it('empty_stack_message', () => {
    setup(emptyStack(() => 'e1'));
    expect(screen.getByText('Empty. Use Add pane.')).toBeInTheDocument();
    expect(screen.queryByRole('tab')).toBeNull();
  });
});
