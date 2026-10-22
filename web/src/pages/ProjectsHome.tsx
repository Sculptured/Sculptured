import { useEffect, useRef, useState } from 'react';
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
import { FinishSetup } from '../setup/FinishSetup';
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
  if (loaded && shell.rows.length === 0) return <main className="page page-wide projects-home"><FinishSetup /><TemplateGallery inline onCreated={created} /></main>;
  return (
    <main className="page page-wide projects-home">
      <div className="list-head-row">
        <h1 className="page-title">Projects</h1>
        <span className="tb-fill" />
        <Link to="/new" className="btn btn-primary" aria-keyshortcuts="n"><Icon name="plus" size={15} />{PROJECT_WORDS.newProject}</Link>
      </div>
      <FinishSetup />
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
  const picks = usePicks(rows.map((r) => r.rig));
  const chosen = rows.filter((r) => picks.has(r.rig));
  const anyLive = chosen.some(running);
  const bulk = v.busy === 'bulk';
  return (
    <div className="table-wrap">
      {chosen.length > 0 && (
        <div className="bulk-bar">
          <span role="status" aria-label="Selected projects">{chosen.length} selected</span>
          <Btn size="sm" disabled={bulk} onClick={() => void v.changeMany(chosen.map((r) => r.rig), 'hidden').then((ok) => ok && picks.clear())}>Hide selected</Btn>
          <Btn size="sm" icon="trash" disabled={bulk || anyLive} onClick={() => void v.changeMany(chosen.map((r) => r.rig), 'trashed').then((ok) => ok && picks.clear())}>Move selected to Trash</Btn>
          <Btn size="sm" variant="ghost" onClick={picks.clear}>Clear selection</Btn>
          {anyLive && <small className="muted">Stop running projects before moving them to the Trash.</small>}
        </div>
      )}
      <table className="data-table projects-table">
        <thead><tr><th className="col-pick"><AllBox label="Select all projects" picks={picks} /></th><th>Name</th><th>Status</th><th className="col-num">Agents</th><th className="col-age">Last activity</th><th className="col-act"><span className="sr-only">Actions</span></th></tr></thead>
        <tbody>
          {rows.map((r) => {
            const summary = shell.summaries?.find((s) => s.id === r.rig);
            const last = lastActivity(r, summary?.updated_at_ms);
            const live = running(r);
            const canStart = !live && shell.isTeam(r.rig);
            return (
              <tr key={r.rig} data-team={r.rig} className={picks.has(r.rig) ? 'picked' : undefined}>
                <td className="col-pick"><input type="checkbox" aria-label={`Select ${r.title}`} checked={picks.has(r.rig)} onChange={() => picks.toggle(r.rig)} /></td>
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
  const trash = usePicks(shell.rows.filter((t) => v.statusOf(t.rig) === 'trashed').map((t) => t.rig));
  return (
    <>
      {(['hidden', 'trashed'] as const).map((status) => {
        const items = shell.rows.filter((t) => v.statusOf(t.rig) === status);
        if (items.length === 0) return null;
        return (
          <details className="project-storage" key={status}>
            <summary>{status === 'hidden' ? 'Hidden projects' : 'Trash'} ({items.length})</summary>
            {status === 'trashed' && <p className="muted">Drafts, files and history are kept.</p>}
            {status === 'trashed' && <TrashBulk items={items.map((t) => t.rig)} picks={trash} v={v} />}
            {items.map((t) => status === 'trashed'
              ? <TrashItem key={t.rig} title={t.title} busy={v.busy === t.rig || v.busy === 'bulk'} onRestore={() => void v.change(t.rig, 'visible')} onDelete={() => void v.deleteForever(t.rig)} picked={trash.has(t.rig)} onPick={() => trash.toggle(t.rig)} />
              : <div key={t.rig} className="stored-row"><span>{t.title}</span><Btn size="sm" disabled={v.busy === t.rig} onClick={() => void v.change(t.rig, 'visible')}>Show</Btn></div>)}
          </details>
        );
      })}
    </>
  );
}

type Picks = { has: (id: string) => boolean; toggle: (id: string) => void; all: () => void; clear: () => void; count: number; total: number };

/** A selection over `ids` (like a mail inbox); ids that leave the list leave the selection. */
function usePicks(ids: string[]): Picks {
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const present = new Set(ids);
  const live = [...picked].filter((id) => present.has(id));
  return {
    has: (id) => picked.has(id) && present.has(id),
    toggle: (id) => setPicked((old) => { const next = new Set(old); if (next.has(id)) next.delete(id); else next.add(id); return next; }),
    all: () => setPicked(new Set(ids)),
    clear: () => setPicked(new Set()),
    count: live.length,
    total: ids.length,
  };
}

/** Ticks everything, or nothing; shows a dash when some are ticked. */
function AllBox({ label, picks }: { label: string; picks: Picks }) {
  const box = useRef<HTMLInputElement>(null);
  const all = picks.total > 0 && picks.count === picks.total;
  useEffect(() => { if (box.current) box.current.indeterminate = picks.count > 0 && !all; });
  return <input ref={box} type="checkbox" aria-label={label} checked={all} disabled={picks.total === 0} onChange={() => (all ? picks.clear() : picks.all())} />;
}

/** The Trash's selection bar: restore, or delete forever after one explicit confirmation. */
function TrashBulk({ items, picks, v }: { items: string[]; picks: Picks; v: ReturnType<typeof useProjectVisibility> }) {
  const [confirming, setConfirming] = useState(false);
  const chosen = items.filter((id) => picks.has(id));
  const bulk = v.busy === 'bulk';
  const n = chosen.length;
  return (
    <div className="bulk-bar">
      <AllBox label="Select everything in the Trash" picks={picks} />
      {n > 0 && <span role="status" aria-label="Selected in the Trash">{n} selected</span>}
      {n > 0 && <Btn size="sm" disabled={bulk} onClick={() => void v.changeMany(chosen, 'visible').then((ok) => ok && picks.clear())}>Restore selected</Btn>}
      {n > 0 && <Btn size="sm" disabled={bulk} onClick={() => setConfirming(true)}>Delete selected forever</Btn>}
      {confirming && n > 0 && (
        <div role="alert" className="trash-confirm">
          <p>Delete {n} {n === 1 ? 'project' : 'projects'} forever? Their designs, runs and history can't be restored. Your project folders are not touched.</p>
          <button className="btn btn-sm tone-red" disabled={bulk} onClick={() => { setConfirming(false); void v.deleteManyForever(chosen).then((ok) => ok && picks.clear()); }}>Yes, delete {n} forever</button>
          <button className="btn btn-sm" onClick={() => setConfirming(false)}>Cancel</button>
        </div>
      )}
    </div>
  );
}
