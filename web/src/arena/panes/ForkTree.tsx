import { Link } from 'react-router-dom';
import { branchNames, branchTree } from '../../tests/model';
import { ApproximateMark, StatusPill, formatWhen } from '../../tests/shared';
import type { Branch, RunSummary } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { runPath } from '../paths';
import { agoWords, branchSeconds, durationWords } from '../words';

/** "Forked from Original at step 5", or how a run's first branch started. */
function forkedFrom(b: Branch, names: Map<string, string>): string {
  if (b.parent_id == null) return 'Started from the beginning';
  const parent = names.get(b.parent_id) ?? 'an earlier branch';
  return b.start_seq != null ? `Forked from ${parent} at step ${b.start_seq}` : `Forked from ${parent}`;
}

function BranchRow({ run, b, depth, names, now }: { run: RunSummary; b: Branch; depth: number; names: Map<string, string>; now: number }) {
  const s = branchSeconds(b);
  return (
    <li style={{ paddingLeft: `calc(${depth} * var(--space-5))` }}>
      <span className="tst-tree" aria-hidden="true">{depth ? '└' : '●'}</span>
      <Link to={runPath(run.id, b.id)}><strong>{names.get(b.id)}</strong></Link>
      <StatusPill status={b.status} short />
      <ApproximateMark of={b} />
      <span className="muted tst-small">{forkedFrom(b, names)}</span>
      <span className="tst-spacer" />
      {s != null && <span className="arena-num tst-small">{durationWords(s)}</span>}
      <span className="arena-num muted tst-small" title={formatWhen(b.created_at)}>{agoWords(b.created_at, now)}</span>
    </li>
  );
}

function Run({ run, now }: { run: RunSummary; now: number }) {
  const names = branchNames(run.branches);
  return (
    <li className="arena-item">
      <div className="arena-item-head">
        <strong>Task {run.task_id}</strong>
        <span className="tst-spacer" />
        <span className="arena-num muted tst-small" title={formatWhen(run.created_at)}>{agoWords(run.created_at, now)}</span>
      </div>
      <ul className="arena-tree" aria-label={`Branches of task ${run.task_id}`}>
        {branchTree(run.branches).map(({ branch: b, depth }) => <BranchRow key={b.id} run={run} b={b} depth={depth} names={names} now={now} />)}
      </ul>
    </li>
  );
}

/** Each run's branches, parent to child: the step each fork started from, how long it ran, when. A failed re-read shows above the last answer. */
function TreeBody() {
  const { runs } = useArena();
  const now = Date.now();
  return (
    <>
      <ArenaProblem error={runs.error} />
      {!runs.value && !runs.error && <p className="muted">Reading runs…</p>}
      {runs.value?.length === 0 && <p className="muted">No runs yet. Start one in Runs; every fork of it shows here, parent to child.</p>}
      {!!runs.value?.length && <ul className="arena-list">{runs.value.map((r) => <Run key={r.id} run={r} now={now} />)}</ul>}
    </>
  );
}

export function ForkTreePane() {
  return <div className="arena-pane"><ArenaGate><TreeBody /></ArenaGate></div>;
}
