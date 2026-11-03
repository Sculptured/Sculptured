import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { Btn } from '../../shell/ui';
import { arena } from '../../tests/api';
import { branchTree } from '../../tests/model';
import { HEALTH_CHANGED, formatWhen } from '../../tests/shared';
import type { RunSummary, TaskSummary } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { comparePath, runPath } from '../paths';
import { StatusCounts } from '../StatusCounts';
import { agoWords, branchSeconds, durationWords } from '../words';
import { NewRun } from './NewRun';

const isRunning = (run: RunSummary) => run.branches.some((b) => b.status === 'running');
const original = (run: RunSummary) => branchTree(run.branches)[0]?.branch;

type Act = { onChanged(): void; onError(e: unknown): void };

/** Delete with a confirm step. After a successful delete the row stays "Deleting…" until the run list drops it. */
function useDelete(run: RunSummary, { onChanged, onError }: Act) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const remove = async () => {
    setBusy(true);
    onError(null);
    try {
      await arena.deleteRun(run.id);
      window.dispatchEvent(new Event(HEALTH_CHANGED));
      onChanged();
    } catch (e) {
      onError(e);
      setBusy(false);
      setConfirming(false);
    }
  };
  return { confirming, busy, remove, ask: () => setConfirming(true), cancel: () => setConfirming(false) };
}

type Deleting = ReturnType<typeof useDelete>;

/** The confirm step, on its own line under the run so the table keeps its width. */
function ConfirmRow({ del, span }: { del: Deleting; span: number }) {
  return (
    <tr className="arena-subrow">
      <td colSpan={span}>
        <span className="tst-confirm" role="group" aria-label="Confirm delete">
          <span className="tst-small">Delete this run, its branches and their saves?</span>
          <Btn size="sm" danger onClick={del.remove} disabled={del.busy}>{del.busy ? 'Deleting…' : 'Delete run'}</Btn>
          <Btn size="sm" variant="ghost" onClick={del.cancel} disabled={del.busy}>Cancel</Btn>
        </span>
      </td>
    </tr>
  );
}

function Actions({ run, del }: { run: RunSummary; del: Deleting }) {
  const finished = run.branches.filter((b) => b.status !== 'running');
  return (
    <td className="arena-actions">
      {finished.length >= 2 && <Link className="btn btn-sm btn-ghost" to={comparePath(finished[0].id, finished[finished.length - 1].id)}>Compare</Link>}
      {isRunning(run) ? <span className="muted tst-small">Can't delete while running</span>
        : !del.confirming && <Btn size="sm" variant="ghost" icon="trash" onClick={del.ask}>Delete</Btn>}
    </td>
  );
}

type RowProps = Act & { run: RunSummary; reason?: string; took: boolean; now: number };

function RunRow({ run, reason, took, now, ...act }: RowProps) {
  const first = original(run);
  const seconds = first ? branchSeconds(first) : null;
  const del = useDelete(run, act);
  return (
    <>
      <tr aria-label={`Run of task ${run.task_id}`}>
        <td className="arena-task">
          {first ? <Link to={runPath(run.id, first.id)}>Task {run.task_id}</Link> : <strong>Task {run.task_id}</strong>}
          {reason && <span className="muted" title={reason}> {reason}</span>}
        </td>
        <td><StatusCounts branches={run.branches} /></td>
        {took && <td className="arena-num" data-label="Took">{seconds != null ? durationWords(seconds) : first?.status === 'running' ? 'running' : '—'}</td>}
        <td className="arena-num" data-label="Started" title={formatWhen(run.created_at)}>{agoWords(run.created_at, now)}</td>
        <Actions run={run} del={del} />
      </tr>
      {del.confirming && <ConfirmRow del={del} span={took ? 5 : 4} />}
    </>
  );
}

/** Every run, newest first, one row each. "Took" is the first run's time, shown when any run has one. */
function RunTable({ runs, tasks, onError }: { runs: RunSummary[]; tasks: TaskSummary[]; onError(e: unknown): void }) {
  const { refreshRuns } = useArena();
  const reason = new Map(tasks.map((t) => [t.id, t.reason]));
  const took = runs.some((r) => { const f = original(r); return !!f && branchSeconds(f) != null; });
  const now = Date.now();
  const sorted = [...runs].sort((a, b) => b.created_at.localeCompare(a.created_at));
  return (
    <div className="arena-table-wrap">
      <table className="arena-table" aria-label="Runs">
        <thead><tr><th>Task</th><th>Branches</th>{took && <th className="arena-num">Took</th>}<th className="arena-num">Started</th><th><span className="sr-only">Actions</span></th></tr></thead>
        <tbody>{sorted.map((r) => <RunRow key={r.id} run={r} reason={reason.get(r.task_id)} took={took} now={now} onChanged={refreshRuns} onError={onError} />)}</tbody>
      </table>
    </div>
  );
}

function useTasks(onError: (e: unknown) => void) {
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  useEffect(() => { arena.tasks().then(setTasks).catch(onError); }, [onError]);
  return tasks;
}

function RunsBody() {
  const { health, runs } = useArena();
  const [error, setError] = useState<unknown>(null);
  const tasks = useTasks(setError);
  return (
    <>
      <NewRun tasks={tasks} openOk={!!health?.open_available} onError={setError} />
      <ArenaProblem error={error} />
      <ArenaProblem error={runs.error} />
      {runs.value?.length === 0 && <p className="muted">No runs yet. Pick a task above and start one. Every step is recorded, so you can rerun from any of them.</p>}
      {!!runs.value?.length && <RunTable runs={runs.value} tasks={tasks} onError={setError} />}
    </>
  );
}

/** Start a run (task and team), open one, or delete one. */
export function RunsPane() {
  return <div className="arena-pane"><ArenaGate><RunsBody /></ArenaGate></div>;
}
