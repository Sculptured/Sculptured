import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Btn } from '../../shell/ui';
import { arena } from '../../tests/api';
import { kindLabel } from '../../tests/team';
import type { TaskSummary, Team } from '../../tests/types';
import { runPath } from '../paths';
import { TeamEditor, useTestTeam } from './TeamEditor';

/** "Triage (Claude Code) → Resolver (Codex)": who works, in order. */
function Chain({ team }: { team: Team | null }) {
  if (!team) return <span className="muted">Loading team…</span>;
  return (
    <span className="arena-chain">
      {team.agents.map((a, i) => (
        <span key={a.id}>
          {i > 0 && <span className="muted" aria-label="hands work to"> → </span>}
          <strong>{a.name}</strong> <span className="muted">({kindLabel(a.kind)})</span>
        </span>
      ))}
    </span>
  );
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

type Props = { tasks: TaskSummary[]; openOk: boolean; onError: (e: unknown) => void };

/** One line: task, the team in order, Start; "Edit team" opens the team below it. */
export function NewRun({ tasks, openOk, onError }: Props) {
  const { team, update, reset } = useTestTeam(onError);
  const [taskId, setTaskId] = useState('');
  const [editing, setEditing] = useState(false);
  const { busy, start } = useStart(team, onError);
  const selected = taskId || tasks[0]?.id || '';
  return (
    <section className="arena-bar-new" aria-label="New run">
      <div className="arena-row">
        <label className="arena-inline">
          <span className="tst-label">Task</span>
          <select value={selected} onChange={(e) => setTaskId(e.target.value)} disabled={busy || tasks.length === 0}>
            {tasks.map((t) => <option key={t.id} value={t.id}>{t.id} · {t.reason}</option>)}
          </select>
        </label>
        <span className="tst-label">Team</span>
        <Chain team={team} />
        <span className="tst-spacer" />
        <Btn size="sm" variant="ghost" aria-expanded={editing} onClick={() => setEditing((e) => !e)} disabled={!team}><span aria-hidden="true">{editing ? '▾' : '▸'}</span> Edit team</Btn>
        <Btn size="sm" variant="primary" icon="play" onClick={() => start(selected)} disabled={busy || !selected || !team}>{busy ? 'Starting…' : 'Start run'}</Btn>
      </div>
      {!busy && tasks.length === 0 && <p className="muted tst-small">Start run waits for a task: Arena lists none yet.</p>}
      {editing && team && <TeamEditor team={team} update={update} reset={reset} openOk={openOk} />}
    </section>
  );
}
