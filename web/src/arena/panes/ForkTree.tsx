import { Link } from 'react-router-dom';
import { branchNames, branchTree } from '../../tests/model';
import { ApproximateMark, StatusPill, formatWhen } from '../../tests/shared';
import type { Branch, RunSummary } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { runPath } from '../paths';

/** "Forked from Original at step 5", or how a run's first branch started. */
function forkedFrom(b: Branch, names: Map<string, string>): string {
  if (b.parent_id == null) return 'Started from the beginning';
  const parent = names.get(b.parent_id) ?? 'an earlier branch';
  return b.start_seq != null ? `Forked from ${parent} at step ${b.start_seq}` : `Forked from ${parent}`;
}

function Run({ run }: { run: RunSummary }) {
  const names = branchNames(run.branches);
  return (
    <li className="arena-item">
      <div className="arena-item-head">
        <strong>Task {run.task_id}</strong>
        <span className="tst-spacer" />
        <span className="muted tst-small">{formatWhen(run.created_at)}</span>
      </div>
      <ul className="arena-tree" aria-label={`Branches of task ${run.task_id}`}>
        {branchTree(run.branches).map(({ branch: b, depth }) => (
          <li key={b.id} style={{ paddingLeft: `calc(${depth} * var(--space-5))` }}>
            <span className="tst-tree" aria-hidden="true">{depth ? '└' : '●'}</span>
            <Link to={runPath(run.id, b.id)}><strong>{names.get(b.id)}</strong></Link>
            <StatusPill status={b.status} short />
            <ApproximateMark of={b} />
            <span className="muted tst-small">{forkedFrom(b, names)}</span>
          </li>
        ))}
      </ul>
    </li>
  );
}

function TreeBody() {
  const { runs, runsError } = useArena();
  if (runsError && !runs) return <ArenaProblem error={runsError} />;
  if (!runs) return <p className="muted">Reading runs…</p>;
  if (!runs.length) return <p className="muted">No runs yet. Start one in Runs; every fork of it shows here, parent to child.</p>;
  return <ul className="arena-list">{runs.map((r) => <Run key={r.id} run={r} />)}</ul>;
}

/** Each run's branches, parent to child, with the step each fork started from. */
export function ForkTreePane() {
  return <div className="arena-pane"><ArenaGate><TreeBody /></ArenaGate></div>;
}
