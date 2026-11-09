import { useCallback, useEffect, useId, useState } from 'react';
import { Link } from 'react-router-dom';
import { apiRequest, newRequestId } from '../api';
import { SUITES_MISSING } from '../control/org/panes';
import { readSuites, type Suites } from '../control/org/data';
import { errorText } from '../errors';
import { Btn } from '../shell/ui';
import { arena } from '../tests/api';
import { useArenaHealth } from '../tests/shared';
import type { TaskSummary } from '../tests/types';
import { ARENA_PATH, runPath } from './paths';
import './runInArena.css';

/**
 * Run in Arena, from a team's board: Sculptured starts an external run in Arena, registers each of
 * the board's agents, and runs the board's own team with each agent connected to Arena's world, as it
 * runs any team; Arena records and scores. The run then shows in Control like any Arena run. A test
 * suite (every task, with repeats) is offered once Arena has suites; until then it says why not.
 */

export type DrivenRun = { run_id: string; branch_id: string; team: string; task_id: string; state: { state: 'driving' | 'finishing' | 'finished' | 'failed'; why?: string; status?: string | null; error?: string | null } };

export const runsOf = (team: string) => apiRequest<{ runs: DrivenRun[] }>('GET', `/api/teams/${encodeURIComponent(team)}/arena-runs`);
const startRun = (team: string, task_id: string) =>
  apiRequest<{ run_id: string; branch_id: string; team: string }>('POST', `/api/teams/${encodeURIComponent(team)}/arena-runs`, { request_id: newRequestId(), task_id });
const finishRun = (branch: string) => apiRequest<DrivenRun>('POST', `/api/arena-runs/${encodeURIComponent(branch)}/finish`, {});

/** A driven run's state as a glyph and words; Arena's own outcome is evidence. */
export function driveWords(s: DrivenRun['state']): { glyph: string; text: string } {
  switch (s.state) {
    case 'driving': return { glyph: '⣟', text: 'running in Shakedown' };
    case 'finishing': return { glyph: '◑', text: `finishing: ${s.why ?? ''}` };
    case 'finished': return { glyph: '■', text: `finished (${s.why ?? 'done'}); Shakedown: ${s.status ?? 'unknown'}${s.error ? `, ${s.error}` : ''}` };
    default: return { glyph: '✕', text: s.why ?? 'failed' };
  }
}

const SUITE_NOT_BUILT = 'Running a test suite from a board is not built yet: Sculptured does not drive Shakedown suite runs yet.';
export const suiteWhy = (s: Suites) => (!s ? 'Asking Shakedown whether it has test suites…' : 'missing' in s ? SUITES_MISSING : 'error' in s ? `Shakedown's test suites could not be read: ${s.error}` : SUITE_NOT_BUILT);

function useBoardRuns(team: string) {
  const [runs, setRuns] = useState<DrivenRun[]>([]);
  const reload = useCallback(() => { runsOf(team).then((r) => setRuns(r.runs)).catch(() => {}); }, [team]);
  useEffect(() => {
    reload();
    const t = setInterval(reload, 3000);
    return () => clearInterval(t);
  }, [reload]);
  return { runs, reload };
}

function RunList({ runs, reload }: { runs: DrivenRun[]; reload(): void }) {
  if (runs.length === 0) return null;
  return (
    <ul className="ria-runs" aria-label="Shakedown runs of this team">
      {runs.map((r) => {
        const w = driveWords(r.state);
        return (
          <li key={r.branch_id}>
            <span aria-hidden="true">{w.glyph}</span> <Link to={runPath(r.run_id, r.branch_id)}>Task {r.task_id}</Link> <span className="muted">{w.text}</span>
            {r.state.state === 'driving' && <Btn size="sm" variant="ghost" onClick={() => { void finishRun(r.branch_id).then(reload); }}>Finish now</Btn>}
          </li>
        );
      })}
    </ul>
  );
}

/** Run a test suite: offered once Sculptured can drive Arena suites; until then disabled, saying why. */
function SuiteButton({ suites }: { suites: Suites }) {
  const ids = useId();
  return (
    <span className="ria-act">
      <Btn variant="ghost" disabled aria-describedby={`${ids}-suite`}>Run a test suite</Btn>
      <span id={`${ids}-suite`} className="reason">{suiteWhy(suites)}</span>
    </span>
  );
}

type FormProps = { team: string; tasks: TaskSummary[]; blocked: string | null; suites: Suites; onStarted(): void };

function Form({ team, tasks, blocked, suites, onStarted }: FormProps) {
  const ids = useId();
  const [task, setTask] = useState(tasks[0]?.id ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const why = blocked ?? (tasks.length === 0 ? 'Shakedown has no tasks to run.' : null);
  const go = async () => {
    setBusy(true);
    setError('');
    try { await startRun(team, task); onStarted(); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  return (
    <div className="ria-form">
      <label className="ria-field">Task
        <select value={task} onChange={(e) => setTask(e.target.value)} disabled={tasks.length === 0}>
          {tasks.map((t) => <option key={t.id} value={t.id}>{t.id} · {t.reason}</option>)}
        </select>
      </label>
      <p className="ria-note">The team's own copy runs with each agent connected to Shakedown's world, on your Claude Code or Codex sign-in (it may cost money). Shakedown records and scores it. It finishes when every agent has finished its turns.</p>
      <span className="ria-act">
        <Btn variant="primary" icon="flask" onClick={() => { void go(); }} disabled={!!why || busy || !task} aria-describedby={why ? `${ids}-why` : undefined}>{busy ? 'Starting…' : 'Start in Shakedown'}</Btn>
        {why && <span id={`${ids}-why`} className="reason">{why}</span>}
      </span>
      <SuiteButton suites={suites} />
      {error && <p className="ria-error" role="alert">▲ {error}</p>}
    </div>
  );
}

/** The panel: Arena's state, the form, and this board's runs. `blocked` says why the design can't run now. */
export function RunInArenaPanel({ team, blocked }: { team: string; blocked: string | null }) {
  const { health, error } = useArenaHealth();
  const [tasks, setTasks] = useState<TaskSummary[] | null>(null);
  const [suites, setSuites] = useState<Suites>(null);
  const { runs, reload } = useBoardRuns(team);
  useEffect(() => {
    if (!health) return;
    arena.tasks().then(setTasks).catch(() => setTasks([]));
    readSuites().then(setSuites).catch((e: unknown) => setSuites({ error: errorText(e) }));
  }, [health]);
  return (
    <div className="ria-panel" role="dialog" aria-label="Run in Shakedown">
      <h2 className="ria-title">Run this team in Shakedown</h2>
      {health === undefined && <p className="muted">Asking Shakedown…</p>}
      {health === null && <p className="ria-error">▲ {errorText(error)} <Link to={ARENA_PATH}>Open Shakedown control</Link></p>}
      {health && tasks && <Form team={team} tasks={tasks} blocked={blocked} suites={suites} onStarted={reload} />}
      <RunList runs={runs} reload={reload} />
    </div>
  );
}

/** The board's "Run in Arena…" button and its panel. */
export function RunInArena({ team, blocked }: { team: string; blocked: string | null }) {
  const [open, setOpen] = useState(false);
  return (
    <span className="ria">
      <Btn size="sm" variant="ghost" icon="flask" aria-expanded={open} onClick={() => setOpen((o) => !o)} tip="Run this team in Shakedown, the staging world, and watch it in Control">Run in Shakedown…</Btn>
      {open && <RunInArenaPanel team={team} blocked={blocked} />}
    </span>
  );
}
