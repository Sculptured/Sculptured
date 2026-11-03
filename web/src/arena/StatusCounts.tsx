import { BRANCH_STATUS } from '../tests/shared';
import type { Branch, BranchStatus } from '../tests/types';

const ORDER: BranchStatus[] = ['running', 'failed', 'infra_error', 'interrupted', 'passed'];
const SHORT: Record<BranchStatus, string> = { running: 'running', failed: 'failed', infra_error: "couldn't run", interrupted: 'stopped', passed: 'passed' };

/** A run's branches by status, each as glyph, count and word: "✕ 1 failed · ✓ 2 passed". */
export function StatusCounts({ branches }: { branches: Pick<Branch, 'status'>[] }) {
  const counts = ORDER.map((s) => ({ s, n: branches.filter((b) => b.status === s).length })).filter((c) => c.n > 0);
  if (!counts.length) return <span className="muted">No branches</span>;
  return (
    <span className="arena-counts">
      {counts.map(({ s, n }, i) => (
        <span key={s} className={`arena-count tst-${BRANCH_STATUS[s].tone}`}>
          {i > 0 && <span className="arena-sep" aria-hidden="true">·</span>}
          <span aria-hidden="true">{BRANCH_STATUS[s].glyph}</span> {n} {SHORT[s]}
        </span>
      ))}
    </span>
  );
}
