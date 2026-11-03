import { describe, expect, it } from 'vitest';
import { applySettings, bringBack, cloneLayout, deleteLayout, orphans, poppedOf, popOutPane, withNothingPopped, withPopped } from './layouts';
import { closePane, panes, type Layout } from './model';
import { slotFor } from './slots';

const counter = () => { let n = 0; return () => `n${++n}`; };
const layout = (): Layout => ({
  id: 'L', name: 'L', popped: [],
  root: { kind: 'stack', id: 's1', panes: [{ id: 'a', type: 'logs', settings: { lines: 5 } }, { id: 'b', type: 'agents' }], active: 0 },
});

describe('layout operations', () => {
  it('pop_out_leaves_a_slot_and_clone_restores_it', () => {
    const out = popOutPane(layout(), 'a', counter());
    expect(out.popped).toEqual([{ id: 'a', type: 'logs', settings: { lines: 5 } }]);
    const slot = panes(out.root)[0];
    expect(slot).toEqual({ id: 'n1', type: 'popped:logs', settings: { instance: 'a' } });
    expect(popOutPane(out, slot.id)).toBe(out);

    const copy = cloneLayout(out, { id: 'C', name: 'C' }, counter());
    expect(copy.popped).toEqual([]);
    expect(panes(copy.root).map((p) => [p.type, p.settings])).toEqual([['logs', { lines: 5 }], ['agents', undefined]]);
    expect(panes(copy.root).some((p) => p.id === 'a' || p.id === 'b')).toBe(false);
  });

  it('bring_back_into_slot_or_first_stack', () => {
    const out = popOutPane(layout(), 'a', counter());
    expect(panes(bringBack(out, 'a').root).map((p) => p.id)).toEqual(['a', 'b']);
    const closed = { ...out, root: closePane(out.root, panes(out.root).find((p) => slotFor(p) === 'a')!.id) };
    const back = bringBack(closed, 'a');
    expect(panes(back.root).map((p) => p.id)).toEqual(['b', 'a']);
    expect(back.popped).toEqual([]);
    expect(bringBack(back, 'a')).toBe(back);
  });

  it('settings_from_a_pop_out_apply_wherever_the_pane_is', () => {
    const out = popOutPane(layout(), 'a', counter());
    expect(applySettings(out, 'a', { lines: 9 }).popped[0].settings).toEqual({ lines: 9 });
    // Already back in the tree (its "back" arrived first): the edit still lands.
    const back = bringBack(out, 'a');
    expect(panes(applySettings(back, 'a', { lines: 9 }).root)[0].settings).toEqual({ lines: 9 });
    expect(applySettings(back, 'gone', { lines: 9 })).toBe(back);
  });

  it('orphans_are_popped_panes_without_a_slot', () => {
    const out = popOutPane(layout(), 'a', counter());
    expect(orphans(out)).toEqual([]);
    expect(orphans({ ...out, root: closePane(out.root, 'n1') })).toEqual(['a']);
  });

  it('delete_keeps_the_last_layout', () => {
    const v = { version: 1 as const, active: 'L', layouts: [layout()] };
    expect(deleteLayout(v, 'L')).toBe(v);
  });

  it('a shared page saves nothing popped, and each channel pops its own panes out again', () => {
    const out = popOutPane(layout(), 'a', counter());
    const v = { version: 1 as const, active: 'L', layouts: [out] };
    const shared = withNothingPopped(v);
    expect(shared.layouts[0].popped).toEqual([]);
    expect(panes(shared.layouts[0].root).map((p) => p.id)).toEqual(['a', 'b']);
    expect(poppedOf(v)).toEqual({ L: ['a'] });
    expect(poppedOf(shared)).toEqual({});
    const again = withPopped(shared, poppedOf(v), counter());
    expect(again.layouts[0].popped.map((p) => p.id)).toEqual(['a']);
    // A pane no longer in the layouts is left alone.
    expect(withPopped(shared, { L: ['gone'], X: ['a'] })).toBe(shared);
  });
});
