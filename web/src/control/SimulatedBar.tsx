import { Link, useLocation } from 'react-router-dom';
import { isArenaDown } from '../arena/data';
import { ArenaDownBanner } from '../arena/gate';
import { ARENA_PATH, runPath } from '../arena/paths';
import { branchTree } from '../tests/model';
import { worldLine } from '../tests/restore';
import { ApproximateMark, StatusPill } from '../tests/shared';
import type { ControlSource, SimulatedRun } from './source';

/**
 * What a simulated run is, above its Control header: the task and branch with its outcome (and an
 * "Approximate restore" mark), the run's other branches to switch to, its world, where a fork
 * started and, when Arena is not answering, how to start it.
 */

function Branches({ a }: { a: SimulatedRun }) {
  if (a.branches.length < 2) return null;
  return (
    <nav className="ctl-sim-branches" aria-label="Branches of this run">
      {branchTree(a.branches).map(({ branch: b, depth }) => (
        <Link key={b.id} to={runPath(a.run, b.id)} className={`ctl-sim-branch${b.id === a.branchId ? ' is-on' : ''}`} aria-current={b.id === a.branchId ? 'page' : undefined}>
          {depth > 0 && <span className="ctl-muted" aria-hidden="true">↳</span>}
          <span>{a.names.get(b.id)}</span>
          <StatusPill status={b.status} short />
          <ApproximateMark of={b} />
        </Link>
      ))}
    </nav>
  );
}

/** Notes under the bar: the fork just started, where this branch was forked from, why it could not finish. */
function Notes({ a }: { a: SimulatedRun }) {
  const notice = (useLocation().state as { notice?: string } | null)?.notice;
  const b = a.branch;
  const parent = b?.parent_id ? a.branches.find((x) => x.id === b.parent_id) : undefined;
  return (
    <>
      {notice && <p className="ctl-sim-note" role="status"><span aria-hidden="true">⑂ </span>{notice}</p>}
      {b?.fork_note && (
        <p className="ctl-sim-note">
          {parent ? <>Fork of <Link to={runPath(a.run, parent.id)}>{a.names.get(parent.id)}</Link>{b.start_seq != null ? ` from step ${b.start_seq}` : ''}: </> : null}{b.fork_note}
        </p>
      )}
      {b?.error && <p className="ctl-sim-note tone-bad"><span aria-hidden="true">⚠ </span>This branch could not finish. <span className="ctl-mono" data-raw-evidence>{b.error}</span></p>}
      {isArenaDown(a.error) && <ArenaDownBanner error={a.error} />}
    </>
  );
}

export function SimulatedBar({ source }: { source: ControlSource }) {
  const a = source.arena!;
  const b = a.branch;
  const world = b ? worldLine(b.events) : '';
  return (
    <div className="ctl-sim">
      <div className="ctl-sim-row">
        <nav className="ctl-sim-crumbs" aria-label="Breadcrumb"><Link to={ARENA_PATH}>Arena</Link><span aria-hidden="true">›</span></nav>
        <h1 className="ctl-sim-title">{source.title}{b && <> <StatusPill status={b.status} /> <ApproximateMark of={b} /></>}</h1>
        <Branches a={a} />
        {world && <span className="ctl-sim-world" title={world}>{world}</span>}
      </div>
      <Notes a={a} />
    </div>
  );
}
