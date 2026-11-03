import type { BranchStatus } from '../tests/types';

/**
 * When two branches can be compared, the same in both Compare panes (Arena's and a run's Control):
 * a branch still running has no final world yet, so the comparison waits until both have finished.
 */
export const MUST_FINISH = '◌ Both branches must finish before they can be compared.';

export const canCompare = (...statuses: (BranchStatus | undefined)[]) => statuses.every((s) => s !== 'running');

/** A branch in a "compare with" list: a running one says so, and cannot be chosen. */
export const choice = (name: string, status: BranchStatus) => ({ label: status === 'running' ? `${name} (running)` : name, disabled: status === 'running' });
