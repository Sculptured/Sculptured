import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Btn, Icon } from '../shell/ui';
import { arena } from './api';
import { branchNames, branchTree } from './model';
import { ApproximateMark, EngineLine, ErrorNote, HEALTH_CHANGED, KindMark, StatusPill, formatWhen, useArenaHealth } from './shared';
import { loadTeam } from './team';
import type { Branch, RunSummary, TaskSummary, Team } from './types';
import { runPath, comparePath } from './paths';

const seconds = (b: Branch) => {
  if (!b.ended_at) return null;
  const s = (Date.parse(b.ended_at) - Date.parse(b.created_at)) / 1000;
  return Number.isNaN(s) ? null : s;
};

/** `/tests`: start a run of the test team on a task, and every run so far with its branches. */
export function TestsHome() {
  const { health, error: healthError } = useArenaHealth();
  const [runs, setRuns] = useState<RunSummary[] | null>(null);
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [team, setTeam] = useState<Team | null>(null);
  const [error, setError] = useState<unknown>(null);

  const refresh = useCallback(() => {
    arena.runs().then((r) => { setRuns(r); setError(null); }).catch(setError);
  }, []);

  useEffect(() => {
    if (!health) return;
    refresh();
    arena.tasks().then(setTasks).catch(setError);
    loadTeam().then(setTeam).catch(setError);
  }, [health != null, refresh]); // eslint-disable-line react-hooks/exhaustive-deps

  const anyRunning = useMemo(() => !!runs?.some((r) => r.branches.some((b) => b.status === 'running')), [runs]);
  useEffect(() => {
    if (!anyRunning) return;
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, [anyRunning, refresh]);

  const reason = useMemo(() => new Map(tasks.map((t) => [t.id, t.reason])), [tasks]);

  return (
    <main className="page page-wide tst-page">
      <header className="tst-head">
        <div>
          <h1 className="page-title"><Icon name="flask" size={20} /> Tests</h1>
          <p className="muted tst-lede">Run a team on a task with its world saved at every step. Go back to any step, change a prompt, model or agent, and rerun from there with the world as it was.</p>
        </div>
        <Link className="btn btn-secondary" to="/tests/team"><Icon name="group" size={15} /> Test team</Link>
      </header>
      {health === undefined && <p className="muted">Asking the test engine…</p>}
      {health === null && <ErrorNote error={healthError} />}
      {health && <EngineLine health={health} />}
      {health && (
        <>
          <NewRun tasks={tasks} team={team} onError={setError} />
          <ErrorNote error={error} />
          <section aria-label="Runs" className="tst-runs">
            <h2 className="tst-h2">Runs {runs && <span className="muted">{runs.length}</span>}</h2>
            {runs?.length === 0 && (
              <div className="tst-empty">
                <Icon name="flask" size={28} />
                <p><strong>No runs yet.</strong></p>
                <p className="muted">Pick a task above and start one. Every step is recorded, so you can rerun from any of them.</p>
              </div>
            )}
            {runs?.map((run) => <RunCard key={run.id} run={run} reason={reason.get(run.task_id)} onChanged={refresh} onError={setError} />)}
          </section>
        </>
      )}
    </main>
  );
}

function NewRun({ tasks, team, onError }: { tasks: TaskSummary[]; team: Team | null; onError: (e: unknown) => void }) {
  const nav = useNavigate();
  const [taskId, setTaskId] = useState('');
  const [busy, setBusy] = useState(false);
  const selected = taskId || tasks[0]?.id || '';
  const start = async () => {
    if (!selected || !team) return;
    setBusy(true);
    onError(null);
    try {
      const r = await arena.startRun(selected, team);
      nav(runPath(r.run_id, r.branch_id));
    } catch (e) {
      onError(e);
      setBusy(false);
    }
  };
  return (
    <section className="tst-card tst-new" aria-label="New run">
      <label className="tst-field tst-grow">
        <span className="tst-label">Task (τ²-bench retail)</span>
        <select value={selected} onChange={(e) => setTaskId(e.target.value)} disabled={busy || tasks.length === 0}>
          {tasks.map((t) => <option key={t.id} value={t.id}>{t.id} · {t.reason}</option>)}
        </select>
      </label>
      <div className="tst-field">
        <span className="tst-label">Team</span>
        <span className="tst-chain">
          {team ? team.agents.map((a, i) => (
            <span key={a.id} className="tst-chain-step">
              {i > 0 && <span className="muted" aria-label="hands work to">→</span>}
              <strong>{a.name}</strong>
              <KindMark kind={a.kind} />
            </span>
          )) : <span className="muted">Loading…</span>}
        </span>
      </div>
      <Btn variant="primary" icon="play" onClick={start} disabled={busy || !selected || !team}>
        {busy ? 'Starting…' : 'Start run'}
      </Btn>
    </section>
  );
}

function RunCard({ run, reason, onChanged, onError }: { run: RunSummary; reason?: string; onChanged: () => void; onError: (e: unknown) => void }) {
  const nav = useNavigate();
  const names = branchNames(run.branches);
  const finished = run.branches.filter((b) => b.status !== 'running');
  const first = branchTree(run.branches)[0]?.branch;
  return (
    <article className="tst-card tst-run">
      <header className="tst-run-head">
        <Link className="tst-run-title" to={first ? runPath(run.id, first.id) : '#'}>Task {run.task_id}</Link>
        {reason && <span className="muted tst-run-reason" title={reason}>{reason}</span>}
        <span className="tst-spacer" />
        <span className="muted tst-small tst-nowrap">{formatWhen(run.created_at)}</span>
        {finished.length >= 2 && (
          <Btn size="sm" icon="grid" onClick={() => nav(comparePath(finished[0].id, finished[finished.length - 1].id))}>Compare</Btn>
        )}
        <DeleteRun run={run} onDone={onChanged} onError={onError} />
      </header>
      <ul className="tst-branches">
        {branchTree(run.branches).map(({ branch: b, depth }) => {
          const s = seconds(b);
          return (
            <li key={b.id}>
              <Link className="tst-branch-row" to={runPath(run.id, b.id)} style={{ paddingLeft: 12 + depth * 22 }}>
                <span className="tst-tree" aria-hidden="true">{depth ? '└' : '●'}</span>
                <strong className="tst-branch-name">{names.get(b.id)}</strong>
                <StatusPill status={b.status} />
                <ApproximateMark of={b} />
                {b.fork_note && <span className="muted tst-fork-note">{b.fork_note}</span>}
                <span className="tst-spacer" />
                <span className="muted tst-small mono">{s != null ? `${s.toFixed(1)} s` : b.status === 'running' ? 'running' : ''}</span>
              </Link>
            </li>
          );
        })}
      </ul>
    </article>
  );
}

/** Delete with an inline confirm; not while a branch of the run is running. */
function DeleteRun({ run, onDone, onError }: { run: RunSummary; onDone: () => void; onError: (e: unknown) => void }) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const running = run.branches.some((b) => b.status === 'running');
  const remove = async () => {
    setBusy(true);
    onError(null);
    try {
      await arena.deleteRun(run.id);
      window.dispatchEvent(new Event(HEALTH_CHANGED));
      onDone();
    } catch (e) {
      onError(e);
      setBusy(false);
      setConfirming(false);
    }
  };
  if (running) return <span className="muted tst-small">Can't delete while running</span>;
  if (!confirming) return <Btn size="sm" variant="ghost" icon="trash" onClick={() => setConfirming(true)}>Delete</Btn>;
  return (
    <span className="tst-confirm">
      <span className="muted tst-small">Delete this run, its branches and their saves?</span>
      <Btn size="sm" danger onClick={remove} disabled={busy}>{busy ? 'Deleting…' : 'Delete'}</Btn>
      <Btn size="sm" variant="ghost" onClick={() => setConfirming(false)} disabled={busy}>Cancel</Btn>
    </span>
  );
}
