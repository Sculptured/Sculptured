import { useId, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { drawnGroups } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { issuesFor, MARK } from '../draft/issues';
import type { DraftIssue } from '../generated/DraftIssue';
import { boardPath } from '../paths';
import { AdvancedFields, ConnectionsField, DeleteAgent, fieldOf, FolderField, JobField, NameField, RunsOnField, type FieldKey } from '../structure/AgentFields';
import { AgentMoreSettings, moreSummary } from '../structure/Panels';
import { SaveStatus, useBoardKeys } from '../structure/StructureBoard';
import type { Project } from './ProjectContext';

const FIELD_LABEL: Record<FieldKey, string> = { name: 'Name', folder: 'Folder', connections: 'Connections', advanced: 'Advanced', other: 'Basics' };

/** One section of the form: a plain heading and its fields, nothing beside it. */
function Section({ id, title, children, one = false }: { id: string; title?: string; children: ReactNode; one?: boolean }) {
  return (
    <section id={id} className={`setup-sec${one ? ' setup-one' : ''}`} aria-label={title}>
      {title && !one && <h2 className="setup-title">{title}</h2>}
      {children}
    </section>
  );
}

/** Every problem, in one compact line at the top; each goes to the field it is about. */
function ProblemLine({ issues, ids }: { issues: DraftIssue[]; ids: string }) {
  if (issues.length === 0) return null;
  const go = (k: FieldKey) => {
    const sec = document.getElementById(`${ids}-${k === 'other' ? 'basics' : k}`);
    const fold = sec?.querySelector<HTMLDetailsElement>(':scope > details');
    if (fold) fold.open = true;
    sec?.scrollIntoView?.({ block: 'center' });
    sec?.querySelector<HTMLElement>('input, textarea, select, button, summary')?.focus();
  };
  // A problem shown beside its field is named here by the field only; the rest are said in full.
  const fields = [...new Set(issues.map(fieldOf).filter((k) => k !== 'other'))];
  const others = issues.filter((i) => fieldOf(i) === 'other');
  const errors = issues.some((i) => i.severity === 'error');
  return (
    <p className={`setup-problems ${errors ? 'tone-red' : 'tone-amber'}`} role="status" aria-label="Problems">
      <span><span aria-hidden="true">{MARK[errors ? 'error' : 'warning']} </span>{issues.length} to fix:</span>
      {fields.map((k) => <button key={k} type="button" className="setup-problem" onClick={() => go(k)}>{FIELD_LABEL[k]}</button>)}
      {others.map((i, n) => <button key={`${i.code}-${n}`} type="button" className="setup-problem" onClick={() => go('other')}>{i.message}</button>)}
    </p>
  );
}

/**
 * The Setup tab: the agent's design as one roomy column (Basics, Job, Folder, Connections, then
 * Advanced folded). It edits the same design as the board: autosaved, and Ctrl+Z undoes.
 */
export function AgentSetup({ p, agentId }: { p: Project; agentId: string | undefined }) {
  const ids = useId();
  const navigate = useNavigate();
  const { t } = p;
  useBoardKeys(t, false);
  const draft = p.draft;
  const a = draft?.agents.find((x) => x.id === agentId);
  if (!draft || !a) {
    return (
      <section className="ap-body setup" aria-label="Setup">
        <p className="muted setup-none">{p.builtHere ? 'Loading…' : 'This team was started from a file, so its setup cannot be edited here.'}</p>
      </section>
    );
  }
  const locked = p.live ? p.running : t.serverLive;
  const mine = issuesFor(p.issues, 'agent', a.id).concat(p.issues.filter((i) => i.element.kind === 'connection' && draft.connections.some((c) => c.id === i.element.id && (c.from === a.id || c.to === a.id))));
  const at = (k: FieldKey) => mine.filter((i) => fieldOf(i) === k);
  const c = { draft, a, edit: t.edit, locked };
  const group = drawnGroups(draft).find((g) => g.id === groupOf(a));
  return (
    <section className="ap-body setup" aria-label="Setup">
      <div className="setup-col">
        <div className="setup-status">
          <ProblemLine issues={mine} ids={ids} />
          {locked && <p className="muted setup-locked">Running: name and job can change; stop the team to change the rest.</p>}
          <SaveStatus state={t.saveState} error={t.saveError} onRetry={() => void t.retry()} onReload={() => void t.reload()} onKeepMine={() => void t.keepMine()} />
        </div>
        <Section id={`${ids}-basics`} title="Basics">
          <NameField a={a} edit={t.edit} issues={at('name')} />
          <RunsOnField {...c} />
        </Section>
        <Section id={`${ids}-job`} title="Job" one><JobField draft={draft} a={a} edit={t.edit} rows={12} /></Section>
        <Section id={`${ids}-folder`} title="Folder" one><FolderField {...c} issues={at('folder')} /></Section>
        <Section id={`${ids}-connections`} title="Connections" one><ConnectionsField {...c} issues={at('connections')} /></Section>
        <div id={`${ids}-advanced`} className="setup-sec setup-advanced">
          <AgentMoreSettings label="Advanced" summary={moreSummary(a, group?.label)}>
            <AdvancedFields {...c} />
          </AgentMoreSettings>
        </div>
        <div className="setup-sec setup-danger">
          <DeleteAgent {...c} onDeleted={() => navigate(boardPath(p.team, 'design'))} />
        </div>
      </div>
    </section>
  );
}
