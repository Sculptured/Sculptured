import { splitOf as split, stackOf as stack, type Node, type PageLayouts } from '../workspace/model';
import type { ControlSource } from './source';

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

/**
 * Process (the control plane): the process view beside Alarms (Approvals and Trends as tabs), and the
 * action timeline across the bottom.
 */
function process(): Node {
  const top = split('process-top', 'row', [
    stack(['process-view', 'process']),
    stack(['process-alarms', 'alarms'], ['process-trends', 'trends'], ['process-approvals', 'approvals']),
  ], [1.6, 1]);
  return split('process', 'col', [top, stack(['process-actions', 'actions'])], [1.5, 1]);
}

/** Agent focus: one agent's detail beside the Timeline over Raw events. */
const focus = (): Node => split('focus', 'row', [stack(['focus-agent', 'agent']), split('focus-side', 'col', [stack(['focus-timeline', 'timeline']), stack(['focus-raw', 'raw'])], [1, 1])], [1, 1]);

/** Cost: Metrics beside Results (a deployment's; a simulated run has no Results). */
function cost(kind: ControlSource['kind']): Node {
  if (kind !== 'deployment') return stack(['cost-metrics', 'metrics']);
  return split('cost', 'row', [stack(['cost-metrics', 'metrics']), stack(['cost-results', 'results'])], [1, 1.4]);
}

/** Replay (a simulated run): the Timeline large, over World state beside Compare. */
const replay = (): Node => split('replay', 'col', [stack(['replay-timeline', 'timeline']), split('replay-bottom', 'row', [stack(['replay-world', 'world']), stack(['replay-compare', 'compare'])], [1, 1])], [1.7, 1]);

/**
 * A simulated run's first-open layouts: Replay first, then Process, Live and Cost (Agent focus and Results
 * are a deployment's: an Arena run's agents have no conversation or results to show there).
 */
function simulatedDefaults(): PageLayouts {
  return {
    version: 1,
    active: 'replay',
    layouts: [
      { id: 'replay', name: 'Replay', root: replay(), popped: [] },
      { id: 'process', name: 'Process', root: process(), popped: [] },
      { id: 'live', name: 'Live', root: live(), popped: [] },
      { id: 'cost', name: 'Cost', root: cost('simulated'), popped: [] },
    ],
  };
}

/** Control's first-open layouts. Pane ids are unique across layouts (a popped-out pane is found by id). */
export function controlDefaults(kind: ControlSource['kind']): PageLayouts {
  if (kind === 'simulated') return simulatedDefaults();
  return {
    version: 1,
    active: 'live',
    layouts: [
      { id: 'live', name: 'Live', root: live(), popped: [] },
      { id: 'process', name: 'Process', root: process(), popped: [] },
      { id: 'focus', name: 'Agent focus', root: focus(), popped: [] },
      { id: 'cost', name: 'Cost', root: cost(kind), popped: [] },
    ],
  };
}
