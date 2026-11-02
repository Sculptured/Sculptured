import { describe, expect, it } from 'vitest';
import { addPane, closePane, dropUnknown, duplicatePane, movePane, panes, resize, setActive, stacks, type Node, type Split, type Stack } from './model';

const pane = (id: string, type = 't') => ({ id, type });
const stack = (id: string, ids: string[]): Stack => ({ kind: 'stack', id, panes: ids.map((i) => pane(i)), active: 0 });
const split = (id: string, dir: 'row' | 'col', kids: Node[], sizes: number[]): Split => ({ kind: 'split', id, dir, kids, sizes });
const counter = () => { let n = 0; return () => `n${++n}`; };

describe('movePane', () => {
  it('move_to_edge_splits', () => {
    const root = split('col', 'col', [stack('a', ['p1']), stack('b', ['p2', 'p3'])], [0.5, 0.5]);
    const out = movePane(root, 'p2', 'a', 'right', counter()) as Split;
    const wrapped = out.kids[0] as Split;
    expect(wrapped.dir).toBe('row');
    expect(wrapped.kids.map((k) => (k as Stack).panes.map((p) => p.id))).toEqual([['p1'], ['p2']]);
    expect(wrapped.sizes).toEqual([0.5, 0.5]);
    expect(out.dir).toBe('col');
  });

  it('move_into_same_direction_split_inserts_sibling', () => {
    const row = split('row', 'row', [stack('a', ['p1']), stack('b', ['p2'])], [0.5, 0.5]);
    const root = split('col', 'col', [row, stack('c', ['p3'])], [0.5, 0.5]);
    const out = movePane(root, 'p3', 'a', 'right', counter()) as Split;
    expect(out.kind).toBe('split');
    const inner = out as Split;
    expect(inner.dir).toBe('row');
    expect(inner.kids.map((k) => (k as Stack).panes[0].id)).toEqual(['p1', 'p3', 'p2']);
    expect(inner.sizes).toEqual([0.25, 0.25, 0.5]);
  });

  it('renormalises sizes when the moved pane leaves the same split', () => {
    const root = split('row', 'row', [stack('a', ['p1']), stack('b', ['p2']), stack('c', ['p3'])], [0.2, 0.3, 0.5]);
    const out = movePane(root, 'p3', 'a', 'right', counter()) as Split;
    expect(out.kids.map((k) => k.id)).toEqual(['a', expect.any(String), 'b']);
    [0.2, 0.2, 0.6].forEach((v, i) => expect(out.sizes[i]).toBeCloseTo(v));
  });

  it('move_center_joins_tabs_and_activates', () => {
    const root = split('row', 'row', [stack('a', ['p1', 'p2']), stack('b', ['p3'])], [0.5, 0.5]);
    const out = movePane(root, 'p3', 'a', 'center') as Stack;
    expect(out.kind).toBe('stack');
    expect(out.panes.map((p) => p.id)).toEqual(['p1', 'p2', 'p3']);
    expect(out.active).toBe(2);
  });

  it('move_onto_itself_is_noop', () => {
    const root = split('row', 'row', [stack('a', ['p1']), stack('b', ['p2', 'p3'])], [0.5, 0.5]);
    for (const zone of ['left', 'right', 'top', 'bottom', 'center'] as const) {
      expect(movePane(root, 'p1', 'a', zone)).toBe(root);
    }
    expect(movePane(root, 'p2', 'b', 'center')).toBe(root);
  });

  it('can split a stack with several tabs by dragging one of its own tabs to an edge', () => {
    const root = stack('a', ['p1', 'p2']);
    const out = movePane(root, 'p2', 'a', 'bottom', counter()) as Split;
    expect(out.dir).toBe('col');
    expect(stacks(out).map((s) => s.panes.map((p) => p.id))).toEqual([['p1'], ['p2']]);
  });
});

describe('closePane', () => {
  it('close_prunes_empty_stack_and_single_child_split', () => {
    const inner = split('inner', 'col', [stack('b', ['p2']), stack('c', ['p3'])], [0.5, 0.5]);
    const root = split('root', 'row', [stack('a', ['p1']), inner], [0.5, 0.5]);
    const out = closePane(root, 'p3') as Split;
    expect(out.kind).toBe('split');
    expect(out.kids.map((k) => k.id)).toEqual(['a', 'b']);
    expect(out.sizes).toEqual([0.5, 0.5]);
    const last = closePane(closePane(out, 'p1'), 'p2') as Stack;
    expect(last.kind).toBe('stack');
    expect(last.panes).toEqual([]);
  });
});

describe('resize', () => {
  it('resize_clamps_min', () => {
    const root = split('s', 'row', [stack('a', ['p1']), stack('b', ['p2'])], [0.5, 0.5]);
    expect((resize(root, 's', 0, 0.1) as Split).sizes).toEqual([0.6, 0.4]);
    const far = resize(root, 's', 0, 0.9) as Split;
    expect(far.sizes[0]).toBeCloseTo(0.85);
    expect(far.sizes[1]).toBeCloseTo(0.15);
    const back = resize(root, 's', 0, -0.9) as Split;
    expect(back.sizes[0]).toBeCloseTo(0.15);
  });
});

describe('other edits', () => {
  it('duplicate_adds_tab_with_new_id', () => {
    const root = stack('a', ['p1', 'p2']);
    const out = duplicatePane(root, 'p1', 'copy') as Stack;
    expect(out.panes.map((p) => p.id)).toEqual(['p1', 'copy', 'p2']);
    expect(out.active).toBe(1);
  });

  it('adds to the target stack or the first stack, and sets the active tab', () => {
    const root = split('s', 'row', [stack('a', ['p1']), stack('b', ['p2'])], [0.5, 0.5]);
    expect(stacks(addPane(root, pane('x'), 'b'))[1].panes.map((p) => p.id)).toEqual(['p2', 'x']);
    expect(stacks(addPane(root, pane('y'), 'missing'))[0].panes.map((p) => p.id)).toEqual(['p1', 'y']);
    expect(setActive(stack('a', ['p1', 'p2']), 'a', 5)).toMatchObject({ active: 1 });
  });

  it('inputs_not_mutated', () => {
    const root = split('row', 'row', [stack('a', ['p1', 'p4']), stack('b', ['p2'])], [0.5, 0.5]);
    const frozen = JSON.stringify(root);
    const deep = (n: Node) => { Object.freeze(n); if (n.kind === 'split') n.kids.forEach(deep); else n.panes.forEach(Object.freeze); };
    deep(root);
    const ids = counter();
    movePane(root, 'p2', 'a', 'center');
    movePane(root, 'p4', 'a', 'top', ids);
    closePane(root, 'p2');
    addPane(root, pane('x'));
    duplicatePane(root, 'p1', 'c');
    resize(root, 'row', 0, 0.1);
    setActive(root, 'a', 1);
    dropUnknown(root, () => false);
    expect(JSON.stringify(root)).toBe(frozen);
    expect(panes(root).length).toBe(3);
  });
});
