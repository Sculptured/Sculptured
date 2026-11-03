import type { Node, PageLayouts } from '../workspace/model';

/** A stack holding one pane; the pane's id is `id`, its stack's `id-s`. */
const stack = (id: string, type: string): Node => ({ kind: 'stack', id: `${id}-s`, panes: [{ id, type }], active: 0 });
const split = (id: string, dir: 'row' | 'col', kids: Node[], sizes: number[]): Node => ({ kind: 'split', id, dir, kids, sizes });

/** Arena control's first-open layouts: Overview (Runs beside World over Engine) and Storage (Saves and disk beside Fork tree). */
export function arenaDefaults(): PageLayouts {
  return {
    version: 1,
    active: 'overview',
    layouts: [
      {
        id: 'overview', name: 'Overview', popped: [],
        root: split('ov', 'row', [stack('ov-runs', 'arena.runs'), split('ov-side', 'col', [stack('ov-world', 'arena.world'), stack('ov-engine', 'arena.engine')], [0.7, 0.3])], [0.45, 0.55]),
      },
      {
        id: 'storage', name: 'Storage', popped: [],
        root: split('st', 'row', [stack('st-storage', 'arena.storage'), stack('st-forks', 'arena.forks')], [0.5, 0.5]),
      },
    ],
  };
}
