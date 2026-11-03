import type { Node, PageLayouts } from '../workspace/model';
import type { ControlSource } from './source';

/** A stack of panes `[id, type]`; its id is the first pane's with `-s`. */
const stack = (...list: [string, string][]): Node => ({ kind: 'stack', id: `${list[0][0]}-s`, panes: list.map(([id, type]) => ({ id, type })), active: 0 });
const split = (id: string, dir: 'row' | 'col', kids: Node[], sizes: number[]): Node => ({ kind: 'split', id, dir, kids, sizes });

/**
 * Live: System overview (the live board, first; Raw events as its tab) beside Status (with
 * Approvals and Alarms as tabs), and the Timeline across the bottom.
 */
function live(): Node {
  const top = split('live-top', 'row', [
    stack(['live-overview', 'overview'], ['live-raw', 'raw']),
    stack(['live-status', 'status'], ['live-approvals', 'approvals'], ['live-alarms', 'alarms']),
  ], [1.6, 1]);
  return split('live', 'col', [top, stack(['live-timeline', 'timeline'])], [2.2, 1]);
}

/** Agent focus: one agent's detail beside the Timeline over Raw events. */
const focus = (): Node => split('focus', 'row', [stack(['focus-agent', 'agent']), split('focus-side', 'col', [stack(['focus-timeline', 'timeline']), stack(['focus-raw', 'raw'])], [1, 1])], [1, 1]);

/** Cost: Metrics beside Results (a deployment's; a simulated run has no Results). */
function cost(kind: ControlSource['kind']): Node {
  if (kind !== 'deployment') return stack(['cost-metrics', 'metrics']);
  return split('cost', 'row', [stack(['cost-metrics', 'metrics']), stack(['cost-results', 'results'])], [1, 1.4]);
}

/** Control's first-open layouts. Pane ids are unique across layouts (a popped-out pane is found by id). */
export function controlDefaults(kind: ControlSource['kind']): PageLayouts {
  return {
    version: 1,
    active: 'live',
    layouts: [
      { id: 'live', name: 'Live', root: live(), popped: [] },
      { id: 'focus', name: 'Agent focus', root: focus(), popped: [] },
      { id: 'cost', name: 'Cost', root: cost(kind), popped: [] },
    ],
  };
}
