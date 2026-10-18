import { useId, useState, type ReactNode } from 'react';
import { reconnect, removeElement, renameGroup, setConnectionKind } from '../draft/edits';
import { groupOf } from '../draft/ids';
import { issuesFor, MARK } from '../draft/issues';
import type { ConnectionKind } from '../generated/ConnectionKind';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import { SaveGroupButton } from '../groups/SaveGroupButton';
import { Btn, Icon } from '../shell/ui';
import { NOT_ENFORCED } from '../vocab';
import { GroupModelFields } from './ModelFields';
import { LOCK_REASON, nameOf, type Edit } from './Panels';

/** A small floating bar over the board for the selected group or arrow: its few settings, nothing stacked beside the board. */
function Bar({ label, children, onClose, locked }: { label: string; children: ReactNode; onClose: () => void; locked: boolean }) {
  const ids = useId();
  return (
    <div className="selection-bar element-bar" role="toolbar" aria-label={label} onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
      {children}
      <span className="sel-sep" aria-hidden="true" />
      <Btn size="sm" variant="ghost" icon="x" iconOnly onClick={onClose} aria-label={`Close ${label.toLowerCase()} settings`} kbd="esc" />
      {locked && <span id={`${ids}-lock`} className="reason sel-reason">{LOCK_REASON}</span>}
    </div>
  );
}

/** A popover button inside a bar. */
function Pop({ label, icon, children, disabled }: { label: string; icon?: 'group' | 'library'; children: ReactNode; disabled?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <span className="sel-item">
      <Btn size="sm" icon={icon} aria-expanded={open} disabled={disabled} onClick={() => setOpen(!open)}>{label} <Icon name="chevron-down" size={12} /></Btn>
      {open && <div className="sel-pop element-pop">{children}</div>}
    </span>
  );
}

/** The selected group: its name, its agents' models, Save to Library and Delete. */
export function GroupBar({ draft, groupId, issues, edit, locked, onClose }: { draft: TeamDraft; groupId: string; issues: DraftIssue[]; edit: Edit; locked: boolean; onClose: () => void }) {
  const ids = useId();
  const g = draft.groups.find((x) => x.id === groupId);
  if (!g) return null;
  const members = draft.agents.filter((a) => groupOf(a) === g.id).length;
  const problems = issuesFor(issues, 'group', g.id);
  return (
    <Bar label="Group" onClose={onClose} locked={locked}>
      <Icon name="group" size={14} className="muted" />
      <label className="sr-only" htmlFor={`${ids}-name`}>Group name</label>
      <input id={`${ids}-name`} className="fld-input bar-input" value={g.label} spellCheck={false} onChange={(e) => edit((d) => renameGroup(d, g.id, e.target.value), `group:${g.id}`)} />
      <span className="muted sel-count">{members} {members === 1 ? 'agent' : 'agents'}</span>
      {problems.length > 0 && <span className="tone-red bar-problem">{MARK.error} {problems.map((i) => i.message).join(' · ')}</span>}
      <Pop label="Models"><GroupModelFields draft={draft} edit={edit} locked={locked} groupId={g.id} /></Pop>
      <Pop label="Library" icon="library"><SaveGroupButton draft={draft} groupId={g.id} /></Pop>
      <Btn size="sm" danger icon="trash" disabled={locked} onClick={() => { if (!edit((d) => removeElement(d, { kind: 'group', id: g.id }))) onClose(); }} tip="Ctrl+Z brings it back">
        Delete group{members > 0 ? ` & ${members} agents` : ''}
      </Btn>
    </Bar>
  );
}

/** The selected arrow: what it means, its kind and Delete. */
export function ConnectionBar({ draft, connectionId, edit, locked, onClose }: { draft: TeamDraft; connectionId: string; edit: Edit; locked: boolean; onClose: () => void }) {
  const ids = useId();
  const c = draft.connections.find((x) => x.id === connectionId);
  if (!c) return null;
  return (
    <Bar label="Connection" onClose={onClose} locked={locked}>
      <Icon name="link" size={14} className="muted" />
      <AgentSelect id={`${ids}-from`} label="Source agent" draft={draft} value={c.from} locked={locked} onChange={(v) => edit((d) => reconnect(d, c.id, 'from', v))} />
      <label className="sr-only" htmlFor={`${ids}-kind`}>Kind</label>
      <select id={`${ids}-kind`} className="fld-input bar-select" value={c.kind} disabled={locked} onChange={(e) => edit((d) => setConnectionKind(d, c.id, e.target.value as ConnectionKind))}>
        <option value="hands-work-to">hands work to</option>
        <option value="reports-to">reports to ({NOT_ENFORCED})</option>
        <option value="watches">watches ({NOT_ENFORCED})</option>
      </select>
      <AgentSelect id={`${ids}-to`} label="Destination agent" draft={draft} value={c.to} locked={locked} onChange={(v) => edit((d) => reconnect(d, c.id, 'to', v))} />
      <Btn size="sm" danger icon="trash" disabled={locked} onClick={() => { edit((d) => removeElement(d, { kind: 'connection', id: c.id })); onClose(); }}>Delete</Btn>
    </Bar>
  );
}

/** Which agent an end of the arrow is on. */
function AgentSelect({ id, label, draft, value, locked, onChange }: { id: string; label: string; draft: TeamDraft; value: string; locked: boolean; onChange: (id: string) => void }) {
  return (
    <>
      <label className="sr-only" htmlFor={id}>{label}</label>
      <select id={id} className="fld-input bar-select bar-who" value={value} disabled={locked} onChange={(e) => onChange(e.target.value)}>
        {draft.agents.map((a) => <option key={a.id} value={a.id}>{nameOf(draft, a.id)}</option>)}
      </select>
    </>
  );
}
