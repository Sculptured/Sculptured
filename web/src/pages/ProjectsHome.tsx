import { useNavigate, Link } from 'react-router-dom';
import { shortAge } from '../herdr';
import { agentsPath, boardPath } from '../paths';
import { DismissibleError } from '../shell/DismissibleError';
import type { TeamRow } from '../shell/model';
import { useShell } from '../shell/ShellContext';
import { TrashItem } from '../shell/TrashItem';
import { Btn, Icon } from '../shell/ui';
import { TemplateGallery } from '../structure/TemplateGallery';
import { hasProcess } from '../status';
import { PROJECT_WORDS } from '../vocab';
import { teamStatus } from './status';
import { StatusBadge } from './StatusBadge';
import { useProjectVisibility } from './useProjectVisibility';

/** The newest thing that happened: an agent's last event, else the design's last edit. */
function lastActivity(r: TeamRow, edited: number | undefined): number | null {
  const latest = Math.max(0, ...r.agents.map((a) => a.lastMs ?? 0));
  return latest > 0 ? latest : (edited ?? null);
}

const running = (r: TeamRow) => r.agents.some((a) => a.seat && (hasProcess(a.seat.lifecycle) || a.seat.lifecycle === 'attention'));

/**
 * `/`: every project as one row: its name, status (shape plus words), agents, last activity and
 * one action. New project is the page's one primary action; with no projects, the templates show here.
 */
export function ProjectsHome() {
  const shell = useShell();
  const v = useProjectVisibility();
  const created = useCreated();
  const loaded = shell.summaries !== null && shell.rigs !== null;
  const rows = shell.rows.filter((r) => v.statusOf(r.rig) === 'visible');
  if (loaded && shell.rows.length === 0) return <main className="page page-wide projects-home"><TemplateGallery inline onCreated={created} /></main>;
  return (
    <main className="page page-wide projects-home">
      <div className="list-head-row">
        <h1 className="page-title">Projects</h1>
        <span className="tb-fill" />
        <Link to="/new" className="btn btn-primary" aria-keyshortcuts="n"><Icon name="plus" size={15} />{PROJECT_WORDS.newProject}</Link>
      </div>
      {shell.listError && <DismissibleError className="tone-red">{PROJECT_WORDS.couldNotLoad}: {shell.listError}</DismissibleError>}
      {v.loadError && <p className="muted">{v.loadError}</p>}
      {v.error && <DismissibleError>{v.error}</DismissibleError>}
      {!loaded && rows.length === 0 ? <p role="status" className="muted">Loading projects…</p> : <ProjectTable rows={rows} v={v} />}
      <Stored v={v} />
    </main>
  );
}

/** A new project opens on its Design board (an imported run opens on its history). */
export function useCreated() {
  const navigate = useNavigate();
  const shell = useShell();
  return (id: string, open?: { history?: boolean }) => {
    shell.reloadTeams();
    navigate(open?.history ? `/p/${encodeURIComponent(id)}/results/history` : boardPath(id, 'design'));
  };
}

function ProjectTable({ rows, v }: { rows: TeamRow[]; v: ReturnType<typeof useProjectVisibility> }) {
  const shell = useShell();
  const navigate = useNavigate();
  const ask = (team: string, action: 'start' | 'stop') => {
    shell.pending.current = { team, action, until: Date.now() + 10_000 };
    navigate(boardPath(team));
  };
  return (
    <div className="table-wrap">
      <table className="data-table projects-table">
        <thead><tr><th>Name</th><th>Status</th><th className="col-num">Agents</th><th className="col-age">Last activity</th><th className="col-act"><span className="sr-only">Actions</span></th></tr></thead>
        <tbody>
          {rows.map((r) => {
            const summary = shell.summaries?.find((s) => s.id === r.rig);
            const last = lastActivity(r, summary?.updated_at_ms);
            const live = running(r);
            const canStart = !live && shell.isTeam(r.rig);
            return (
              <tr key={r.rig} data-team={r.rig}>
                <td><Link to={boardPath(r.rig)} className="pt-name">{r.title}</Link></td>
                <td><StatusBadge s={teamStatus(r)} /></td>
                <td className="col-num"><Link to={agentsPath(r.rig)} className="muted" aria-label={`${r.agents.length} agents in ${r.title}`}>{r.agents.length}</Link></td>
                <td className="col-age muted">{last ? `${shortAge(last, shell.now)} ago` : 'unknown'}</td>
                <td className="col-act">
                  {live && <Btn size="sm" icon="stop" onClick={() => ask(r.rig, 'stop')}>Stop…</Btn>}
                  {canStart && <Btn size="sm" icon="play" onClick={() => ask(r.rig, 'start')}>Start…</Btn>}
                  <details className="project-menu top-menu">
                    <summary aria-label={`Manage ${r.title}`} data-tip="Project actions"><Icon name="more" size={15} /></summary>
                    <div className="top-menu-body">
                      <button type="button" disabled={v.busy === r.rig} onClick={() => void v.change(r.rig, 'hidden')}>Hide</button>
                      <button type="button" disabled={v.busy === r.rig || live} onClick={() => void v.change(r.rig, 'trashed')}>Move to Trash</button>
                      {live && <small>Stop the team before moving it to the Trash.</small>}
                    </div>
                  </details>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {rows.length === 0 && <p className="muted empty-line">{PROJECT_WORDS.noProjects}</p>}
    </div>
  );
}

/** Hidden projects and the Trash, one step back: each can be shown or restored. */
function Stored({ v }: { v: ReturnType<typeof useProjectVisibility> }) {
  const shell = useShell();
  return (
    <>
      {(['hidden', 'trashed'] as const).map((status) => {
        const items = shell.rows.filter((t) => v.statusOf(t.rig) === status);
        if (items.length === 0) return null;
        return (
          <details className="project-storage" key={status}>
            <summary>{status === 'hidden' ? 'Hidden projects' : 'Trash'} ({items.length})</summary>
            {status === 'trashed' && <p className="muted">Drafts, files and history are kept.</p>}
            {items.map((t) => status === 'trashed'
              ? <TrashItem key={t.rig} title={t.title} busy={v.busy === t.rig} onRestore={() => void v.change(t.rig, 'visible')} onDelete={() => void v.deleteForever(t.rig)} />
              : <div key={t.rig} className="stored-row"><span>{t.title}</span><Btn size="sm" disabled={v.busy === t.rig} onClick={() => void v.change(t.rig, 'visible')}>Show</Btn></div>)}
          </details>
        );
      })}
    </>
  );
}
