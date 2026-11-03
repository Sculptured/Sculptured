import { useState } from 'react';
import { Link } from 'react-router-dom';
import { plural } from '../../format';
import { Btn } from '../../shell/ui';
import { arena } from '../../tests/api';
import { branchTree } from '../../tests/model';
import { HEALTH_CHANGED, StatusPill, formatWhen } from '../../tests/shared';
import type { RunSummary } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { comparePath, runPath } from '../paths';
import { NewRun } from './NewRun';

const isRunning = (run: RunSummary) => run.branches.some((b) => b.status === 'running');

/** Delete with a confirm step; not while a branch of the run is running. */
function DeleteRun({ run, onDone, onError }: { run: RunSummary; onDone(): void; onError(e: unknown): void }) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const remove = async () => {
    setBusy(true);
    onError(null);
    try {
      await arena.deleteRun(run.id);
      window.dispatchEvent(new Event(HEALTH_CHANGED));
      onDone();
    } catch (e) {
      onError(e);
    }
    setBusy(false);
    setConfirming(false);
  };
  if (isRunning(run)) return <span className="muted tst-small">Can't delete while running</span>;
  if (!confirming) return <Btn size="sm" variant="ghost" icon="trash" onClick={() => setConfirming(true)}>Delete</Btn>;
  return (
    <span className="tst-confirm" role="group" aria-label="Confirm delete">
      <span className="tst-small">Delete this run, its branches and their saves?</span>
      <Btn size="sm" danger onClick={remove} disabled={busy}>{busy ? 'Deleting…' : 'Delete run'}</Btn>
      <Btn size="sm" variant="ghost" onClick={() => setConfirming(false)} disabled={busy}>Cancel</Btn>
    </span>
  );
}

function RunRow({ run, onChanged, onError }: { run: RunSummary; onChanged(): void; onError(e: unknown): void }) {
  const tree = branchTree(run.branches);
  const first = tree[0]?.branch;
  const latest = tree.length ? [...run.branches].sort((a, b) => b.created_at.localeCompare(a.created_at))[0] : undefined;
  const finished = run.branches.filter((b) => b.status !== 'running');
  return (
    <li className="arena-item">
      <div className="arena-item-head">
        {first ? <Link className="tst-run-title" to={runPath(run.id, first.id)}>Task {run.task_id}</Link> : <strong>Task {run.task_id}</strong>}
        {latest && <StatusPill status={latest.status} short />}
        <span className="muted tst-small">{plural(run.branches.length, 'branch', 'branches')} · {formatWhen(run.created_at)}</span>
        <span className="tst-spacer" />
        {finished.length >= 2 && <Link className="btn btn-sm btn-ghost" to={comparePath(finished[0].id, finished[finished.length - 1].id)}>Compare</Link>}
        <DeleteRun run={run} onDone={onChanged} onError={onError} />
      </div>
    </li>
  );
}

function RunsBody() {
  const { health, runs, runsError, refreshRuns } = useArena();
  const [error, setError] = useState<unknown>(null);
  return (
    <>
      <NewRun openOk={!!health?.open_available} onError={setError} />
      <ArenaProblem error={error} />
      {runsError != null && <ArenaProblem error={runsError} />}
      {runs?.length === 0 && <p className="muted">No runs yet. Pick a task above and start one. Every step is recorded, so you can rerun from any of them.</p>}
      {runs && runs.length > 0 && (
        <ul className="arena-list" aria-label="Runs">
          {runs.map((r) => <RunRow key={r.id} run={r} onChanged={refreshRuns} onError={setError} />)}
        </ul>
      )}
    </>
  );
}

/** Start a run (task and team), open one, or delete one. */
export function RunsPane() {
  return <div className="arena-pane"><ArenaGate><RunsBody /></ArenaGate></div>;
}
