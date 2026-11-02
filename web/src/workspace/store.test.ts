import { beforeEach, describe, expect, it } from 'vitest';
import { saveLayouts, loadLayouts } from './store';
import type { PageLayouts } from './model';

const defaults = (): PageLayouts => ({
  version: 1,
  active: 'default',
  layouts: [{ id: 'default', name: 'Default', root: { kind: 'stack', id: 's', panes: [{ id: 'd', type: 'home' }], active: 0 }, popped: [] }],
});
const known = (type: string) => type !== 'gone';
const saved = (): PageLayouts => ({
  version: 1,
  active: 'mine',
  layouts: [{ id: 'mine', name: 'Mine', root: { kind: 'stack', id: 's', panes: [{ id: 'a', type: 'chat' }, { id: 'b', type: 'gone' }], active: 1 }, popped: [{ id: 'c', type: 'gone' }] }],
});

beforeEach(() => localStorage.clear());

describe('workspace store', () => {
  it('loads_with_unknown_pane_dropped', () => {
    saveLayouts('control', saved());
    const out = loadLayouts('control', defaults, known);
    expect(out.active).toBe('mine');
    expect(out.layouts[0].root).toMatchObject({ kind: 'stack', panes: [{ id: 'a', type: 'chat' }], active: 0 });
    expect(out.layouts[0].popped).toEqual([]);
  });

  it('corrupt_value_falls_back', () => {
    const key = 'sculptured.workspace.v1.control';
    localStorage.setItem(key, '{not json');
    expect(loadLayouts('control', defaults, known)).toEqual(defaults());
    localStorage.setItem(key, JSON.stringify({ ...saved(), version: 2 }));
    expect(loadLayouts('control', defaults, known)).toEqual(defaults());
    const { active: _active, ...noActive } = saved();
    localStorage.setItem(key, JSON.stringify(noActive));
    expect(loadLayouts('control', defaults, known)).toEqual(defaults());
    localStorage.setItem(key, JSON.stringify({ ...saved(), active: 'nope' }));
    expect(loadLayouts('control', defaults, known)).toEqual(defaults());
    localStorage.setItem(key, JSON.stringify({ ...saved(), layouts: [{ id: 'mine', name: 'x', root: { kind: 'bogus' }, popped: [] }] }));
    expect(loadLayouts('control', defaults, known)).toEqual(defaults());
  });

  it('save_then_load_roundtrip_per_page', () => {
    const other = { ...defaults(), active: 'default' };
    const mine = { ...saved(), layouts: [{ ...saved().layouts[0], popped: [], root: { kind: 'stack' as const, id: 's', panes: [{ id: 'a', type: 'chat' }], active: 0 } }] };
    saveLayouts('control', mine);
    saveLayouts('arena', other);
    expect(loadLayouts('control', defaults, known)).toEqual(mine);
    expect(loadLayouts('arena', () => mine, known)).toEqual(other);
    expect(localStorage.getItem('sculptured.workspace.v1.control')).not.toBeNull();
  });
});
