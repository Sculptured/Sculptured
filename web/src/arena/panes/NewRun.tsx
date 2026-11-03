import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Btn } from '../../shell/ui';
import { arena } from '../../tests/api';
import { KindMark } from '../../tests/shared';
import type { TaskSummary, Team } from '../../tests/types';
import { runPath } from '../paths';
import { TeamEditor, useTestTeam } from './TeamEditor';

function Chain({ team }: { team: Team | null }) {
  if (!team) return <span className="muted">Loading…</span>;
  return (
    <span className="tst-chain">
      {team.agents.map((a, i) => (
        <span key={a.id} className="tst-chain-step">
          {i > 0 && <span className="muted" aria-label="hands work to">→</span>}
          <strong>{a.name}</strong>
          <KindMark kind={a.kind} />
        </span>
      ))}
    </span>
  );
}

function useTasks(onError: (e: unknown) => void) {
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  useEffect(() => { arena.tasks().then(setTasks).catch(onError); }, [onError]);
  return tasks;
}

/** Starts the team on the chosen task, then opens the new branch. */
function useStart(team: Team | null, onError: (e: unknown) => void) {
  const nav = useNavigate();
  const [busy, setBusy] = useState(false);
  const start = async (task: string) => {
    if (!task || !team) return;
    setBusy(true);
    onError(null);
    try {
      const r = await arena.startRun(task, team);
      nav(runPath(r.run_id, r.branch_id));
    } catch (e) {
      onError(e);
      setBusy(false);
    }
  };
  return { busy, start };
}

/** Pick a task, check (or edit) the team, start. */
export function NewRun({ openOk, onError }: { openOk: boolean; onError: (e: unknown) => void }) {
  const tasks = useTasks(onError);
  const { team, update, reset } = useTestTeam(onError);
  const [taskId, setTaskId] = useState('');
  const [editing, setEditing] = useState(false);
  const { busy, start } = useStart(team, onError);
  const selected = taskId || tasks[0]?.id || '';
  return (
    <section className="tst-card arena-new" aria-label="New run">
      <label className="tst-field">
        <span className="tst-label">Task</span>
        <select value={selected} onChange={(e) => setTaskId(e.target.value)} disabled={busy || tasks.length === 0}>
          {tasks.map((t) => <option key={t.id} value={t.id}>{t.id} · {t.reason}</option>)}
        </select>
      </label>
      <div className="tst-field">
        <span className="tst-label">Team</span>
        <Chain team={team} />
      </div>
      <div className="arena-row">
        <Btn variant="primary" icon="play" onClick={() => start(selected)} disabled={busy || !selected || !team}>{busy ? 'Starting…' : 'Start run'}</Btn>
        <Btn variant="ghost" icon="pencil" aria-expanded={editing} onClick={() => setEditing((e) => !e)} disabled={!team}>{editing ? 'Done editing team' : 'Edit team'}</Btn>
        {!busy && tasks.length === 0 && <span className="muted tst-small">No tasks yet: Arena lists none.</span>}
      </div>
      {editing && team && <TeamEditor team={team} update={update} reset={reset} openOk={openOk} />}
    </section>
  );
}
