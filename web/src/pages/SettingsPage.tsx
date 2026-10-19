import { useId, useState, type ReactNode } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { renameTeam } from '../draft/edits';
import { drawnGroups } from '../draft/geometry';
import { plural } from '../format';
import { workflowPath } from '../paths';
import { DismissibleError } from '../shell/DismissibleError';
import { Btn } from '../shell/ui';
import { BudgetFields } from '../structure/BudgetFields';
import { ClaudeSetupFields } from '../structure/ClaudeSetupFields';
import { ImproveFromRun } from '../structure/ImproveFromRun';
import { TeamModelFields } from '../structure/ModelFields';
import { OriginLine } from '../structure/Panels';
import { ProjectFolder } from '../structure/ProjectFolder';
import { ProtectionFields } from '../structure/ProtectionFields';
import { TeamExport } from '../structure/TeamAdvanced';
import { TeamLimitsFields } from '../structure/TeamLimitsFields';
import { TraceSettings } from '../structure/TraceSettings';
import { useProject, type Project } from './ProjectContext';
import { ComponentsSection } from '../structure/ComponentsSection';
import { liveFacts } from './useProjectRun';
import { useProjectVisibility } from './useProjectVisibility';

/** One settings group: a heading, an optional short hint, its controls. */
function Group({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="set-sec" aria-label={title}>
      <h2 className="set-title">{title}</h2>
      {hint && <p className="muted set-hint">{hint}</p>}
      {children}
    </section>
  );
}

/**
 * `/p/:team/settings`: the team as a whole. Name and folder, default models, run budget,
 * workflow, git, export, and Hide or Move to Trash. Edits autosave like the board.
 */
export function SettingsPage() {
  const p = useProject();
  const draft = p.draft;
  if (!draft) {
    return (
      <main className="page page-form settings-page">
        {p.builtHere && !p.t.loadError ? <p role="status" className="muted">Loading settings…</p> : <p className="muted">This team was started from a file, so its settings live in that file.</p>}
        <DangerZone p={p} />
      </main>
    );
  }
  const locked = p.live ? p.running : p.t.serverLive;
  const c = { draft, edit: p.t.edit, locked };
  return (
    <main className="page page-form settings-page">
      <General p={p} locked={locked} />
      <Group title="Default models" hint="Each agent uses these unless its group or its own setup picks another."><TeamModelFields {...c} /></Group>
      <Group title="Run budget"><BudgetFields {...c} /></Group>
      <Shortcuts p={p} />
      <MoreSettings p={p} locked={locked} />
      <Group title="Export"><TeamExport teamId={p.team} draft={draft} flush={p.t.flush} /></Group>
      <DangerZone p={p} />
    </main>
  );
}

/** Links to the team's workflow pages (when it has a workflow) and its Git changes. */
function Shortcuts({ p }: { p: Project }) {
  return (
    <>
      {p.draft?.workflow && (
        <Group title="Workflow">
          <p className="set-links">
            <Link className="btn" to={workflowPath(p.team, 'input')}>Input</Link>
            <Link className="btn" to={workflowPath(p.team, 'settings')}>Dispatch &amp; composition</Link>
            <Link className="btn" to={workflowPath(p.team, 'list')}>Workflow list</Link>
          </p>
        </Group>
      )}
      <Group title="Git">
        <p className="set-links"><Link className="btn" to={`/p/${encodeURIComponent(p.team)}/git`}>Open Git changes</Link></p>
      </Group>
    </>
  );
}

/** Under "More settings": components, setup, protection, limits, improve from a run, send traces. */
function MoreSettings({ p, locked }: { p: Project; locked: boolean }) {
  const c = { draft: p.draft!, edit: p.t.edit, locked };
  return (
    <details className="set-sec set-more">
      <summary><h2 className="set-title">More settings</h2> <span className="muted">· components, setup, protection, limits, improve from a run, send traces</span></summary>
      <ComponentsSection draft={p.draft!} edit={p.t.edit} locked={locked} />
      <ClaudeSetupFields {...c} />
      <ProtectionFields {...c} />
      <TeamLimitsFields {...c} />
      <ImproveFromRun teamId={p.team} draft={p.draft!} edit={p.t.edit} locked={locked} />
      <TraceSettings teamId={p.team} />
    </details>
  );
}

function General({ p, locked }: { p: Project; locked: boolean }) {
  const ids = useId();
  const draft = p.draft!;
  return (
    <Group title="Team">
      {p.t.origin && <OriginLine origin={p.t.origin} notRunYet={p.t.startedRevision === null} />}
      <div className="fld">
        <label className="fld-label" htmlFor={`${ids}-name`}>Team name</label>
        <input id={`${ids}-name`} className="fld-input" value={draft.name} spellCheck={false} onChange={(e) => p.t.edit((d) => renameTeam(d, e.target.value), 'team-name')} />
      </div>
      <p className="muted set-hint">{plural(draft.agents.length, 'agent')} · {plural(draft.connections.length, 'connection')} · {plural(drawnGroups(draft).length, 'group')}</p>
      <ProjectFolder draft={draft} edit={p.t.edit} locked={locked} />
    </Group>
  );
}

/** Hide the project from lists, or move it to the Trash (restorable from Projects). */
function DangerZone({ p }: { p: Project }) {
  const v = useProjectVisibility();
  const navigate = useNavigate();
  const [asking, setAsking] = useState(false);
  const running = liveFacts(p).live;
  const away = (status: 'hidden' | 'trashed') => void v.change(p.team, status).then((ok) => ok && navigate('/'));
  return (
    <Group title="Remove" hint="Hidden projects and the Trash are listed under Projects; both can be restored.">
      <div className="set-links">
        <Btn onClick={() => away('hidden')} disabled={v.busy === p.team}>Hide project</Btn>
        {asking ? (
          <span role="group" aria-label="Confirm move to Trash" className="confirm">
            <span>Move {p.title} to the Trash? Its design, runs and history are kept.</span>
            <Btn danger onClick={() => away('trashed')} disabled={v.busy === p.team}>Move to Trash</Btn>
            <Btn onClick={() => setAsking(false)}>Cancel</Btn>
          </span>
        ) : (
          <Btn danger icon="trash" onClick={() => setAsking(true)} disabled={running} aria-describedby={running ? 'trash-why' : undefined}>Move to Trash…</Btn>
        )}
        {running && <span id="trash-why" className="reason">Stop the team first.</span>}
      </div>
      {v.error && <DismissibleError>{v.error}</DismissibleError>}
    </Group>
  );
}
