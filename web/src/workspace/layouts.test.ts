import { describe, expect, it } from 'vitest';
import { bringBack, cloneLayout, deleteLayout, popOutPane } from './layouts';
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

  it('delete_keeps_the_last_layout', () => {
    const v = { version: 1 as const, active: 'L', layouts: [layout()] };
    expect(deleteLayout(v, 'L')).toBe(v);
  });
});
