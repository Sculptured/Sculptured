import type { TeamOrigin } from '../generated/TeamOrigin';
import { SaveGroupButton } from '../groups/SaveGroupButton';
import { useId, useState, type ReactNode } from 'react';
import type { ConnectionKind } from '../generated/ConnectionKind';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import { vendor } from '../herdr';
import { reconnect, removeElement, renameGroup, setConnectionKind, type Selection } from '../draft/edits';
import { drawnGroups } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { issuesFor, MARK } from '../draft/issues';
import { Btn, Icon, VendorMark } from '../shell/ui';
import type { EditResult } from '../useTeamDraft';
import { CONNECTION_WORDS, NOT_ENFORCED } from '../vocab';
import { GroupModelFields } from './ModelFields';
import { AdvancedFields, ConnectionsField, DeleteAgent, Field, FolderField, JobField, NameField, RunsOnField } from './AgentFields';

export type Edit = (fn: (d: TeamDraft) => EditResult, key?: string) => string | null;

/** Why structure can't change while the team runs. */
export const LOCK_REASON = 'Stop the team to change this.';

export const kindVendor = (k: 'claude' | 'codex' | 'practice') =>
  vendor(k === 'claude' ? 'claude-code' : k === 'codex' ? 'codex' : 'stub');

export const nameOf = (d: TeamDraft, id: string) => d.agents.find((a) => a.id === id)?.name.trim() || 'Unnamed agent';

export function connectionSentence(d: TeamDraft, c: { from: string; to: string; kind: ConnectionKind }): string {
  const s = `${nameOf(d, c.from)} ${CONNECTION_WORDS[c.kind]} ${nameOf(d, c.to)}`;
  return c.kind === 'hands-work-to' ? s : `${s} (${NOT_ENFORCED})`;
}

/** A box-drawn side panel with its title in the top border. */
function SidePanel({ title, label, children, tools }: { title: ReactNode; label: string; children: ReactNode; tools?: ReactNode }) {
  return (
    <section className="panel side-panel" aria-label={label}>
      <div className="panel-bar">
        <span className="panel-title">{title}</span>
        <span className="panel-rule" aria-hidden="true" />
        {tools && <span className="panel-tools">{tools}</span>}
      </div>
      <div className="side-body">{children}</div>
    </section>
  );
}

function Problems({ issues }: { issues: DraftIssue[] }) {
  if (issues.length === 0) return null;
  return (
    <ul className="side-problems" aria-label="Problems">
      {issues.map((i) => (
        <li key={`${i.code}-${i.message}`} className={i.severity === 'error' ? 'tone-red' : 'tone-amber'}>
          <span aria-hidden="true">{MARK[i.severity]} </span>
          {i.message}
        </li>
      ))}
    </ul>
  );
}

type AgentPanelProps = {
  draft: TeamDraft;
  agentId: string;
  issues: DraftIssue[];
  edit: Edit;
  locked: boolean;
  onSelect: (s: Selection | null) => void;
};

const AGENT_MORE_KEY = 'sculptured.agent.more-settings.v1';
function readAgentMore(): boolean {
  try { return globalThis.localStorage?.getItem(AGENT_MORE_KEY) === 'true'; } catch { return false; }
}

/** What is set under More settings, so a closed disclosure still says what it holds. */
export function moreSummary(a: TeamDraft['agents'][number], group: string | undefined): string {
  const set: string[] = [];
  if (group) set.push(`group ${group.trim() || 'unnamed'}`);
  if (a.advanced.token_limit || a.advanced.turn_limit || a.advanced.minute_limit) set.push('limits set');
  if (a.advanced.check) set.push('check set');
  if (a.advanced.briefing === 'minimal') set.push('minimal briefing');
  return set.length ? set.join(' · ') : 'group, limits, check';
}

/** Group, limits, check and Advanced: one step back from an agent's essentials. Remembers whether it is open. */
export function AgentMoreSettings({ summary, children, label = 'More settings' }: { summary: string; children: ReactNode; label?: string }) {
  const [open, setOpen] = useState(readAgentMore);
  return (
    <details className="ux-agent-more" open={open} onToggle={(e) => {
      const next = e.currentTarget.open;
      setOpen(next);
      try { localStorage.setItem(AGENT_MORE_KEY, String(next)); } catch { /* Per-browser convenience only. */ }
    }}>
      <summary>{label} <span className="muted">· {summary}</span></summary>
      <div className="ux-agent-more-body">{children}</div>
    </details>
  );
}

/** An agent of a saved group in the Library's group editor (projects open an agent on its own page). */
export function AgentPanel({ draft, agentId, issues, edit, locked, onSelect }: AgentPanelProps) {
  const a = draft.agents.find((x) => x.id === agentId)!;
  const v = kindVendor(a.kind);
  const inGroup = drawnGroups(draft).find((g) => g.id === groupOf(a));
  const c = { draft, a, edit, locked };
  return (
    <SidePanel
      label={`Agent ${a.name || 'Unnamed agent'}`}
      title={<><VendorMark v={v} size="sm" /> <span className={`team-title ${v.cls}`}>{a.name || 'Unnamed agent'}</span></>}
      tools={<Btn variant="ghost" icon="x" iconOnly onClick={() => onSelect(null)} aria-label="Close agent panel" kbd="esc" />}
    >
      <Problems issues={issuesFor(issues, 'agent', a.id)} />
      <RunsOnField {...c} />
      <NameField a={a} edit={edit} />
      <JobField draft={draft} a={a} edit={edit} />
      <FolderField {...c} />
      <ConnectionsField {...c} />
      <AgentMoreSettings summary={moreSummary(a, inGroup?.label)}>
        <AdvancedFields {...c} />
      </AgentMoreSettings>
      <DeleteAgent {...c} onDeleted={() => onSelect(null)} />
    </SidePanel>
  );
}

export function GroupPanel({ draft, groupId, issues, edit, locked, onSelect, librarySave = true }: { librarySave?: boolean; draft: TeamDraft; groupId: string; issues: DraftIssue[]; edit: Edit; locked: boolean; onSelect: (s: Selection | null) => void }) {
  const ids = useId();
  const g = draft.groups.find((x) => x.id === groupId)!;
  const members = draft.agents.filter((a) => groupOf(a) === g.id);
  const delReason = locked ? LOCK_REASON : !librarySave ? 'A reusable block must keep its group.' : null;
  return (
    <SidePanel
      label={`Group ${g.label}`}
      title={
        <>
          <Icon name="group" size={16} className="muted" /> <span className="eyebrow">Group</span> <span className="team-title">{g.label || 'Unnamed group'}</span>
        </>
      }
      tools={
        <Btn variant="ghost" icon="x" iconOnly onClick={() => onSelect(null)} aria-label="Close group panel" kbd="esc" />
      }
    >
      <Problems issues={issuesFor(issues, 'group', g.id)} />
      <GroupModelFields draft={draft} edit={edit} locked={locked} groupId={g.id} />
      {librarySave && <SaveGroupButton draft={draft} groupId={g.id} />}
      <Field label="Name" htmlFor={`${ids}-name`}>
        <input id={`${ids}-name`} className="fld-input" value={g.label} spellCheck={false} onChange={(e) => edit((d) => renameGroup(d, g.id, e.target.value), `group:${g.id}`)} />
      </Field>
      <Field label={`Agents (${members.length})`}>
        <ul className="conn-list">
          {members.map((a) => (
            <li key={a.id}>
              <button type="button" className="link" onClick={() => onSelect({ kind: 'agent', id: a.id })}>
                <VendorMark v={kindVendor(a.kind)} size="xs" /> {a.name || 'Unnamed agent'}
              </button>
            </li>
          ))}
          {members.length === 0 && <li className="muted">No agents yet. Drag one in, or use Move to group… on an agent.</li>}
        </ul>
      </Field>
      <div className="fld-actions">
        <Btn danger icon="trash" disabled={!!delReason} aria-describedby={delReason ? `${ids}-del` : undefined} onClick={() => {
          if (!edit((d) => removeElement(d, { kind: 'group', id: g.id }))) onSelect(null);
        }}>
          Delete group{members.length > 0 ? ` & ${members.length} agents` : ''}
        </Btn>
        {delReason && (
          <span id={`${ids}-del`} className="reason">
            {delReason}
          </span>
        )}
      </div>
    </SidePanel>
  );
}

export function ConnectionPanel({ draft, connectionId, issues, edit, locked, onSelect }: { draft: TeamDraft; connectionId: string; issues: DraftIssue[]; edit: Edit; locked: boolean; onSelect: (s: Selection | null) => void }) {
  const ids = useId();
  const c = draft.connections.find((x) => x.id === connectionId)!;
  return (
    <SidePanel
      label="Connection"
      title={
        <>
          <Icon name="link" size={16} className="muted" /> Connection
        </>
      }
      tools={
        <Btn variant="ghost" icon="x" iconOnly onClick={() => onSelect(null)} aria-label="Close connection panel" kbd="esc" />
      }
    >
      <Problems issues={issuesFor(issues, 'connection', c.id)} />
      <p className="conn-sentence">{connectionSentence(draft, c)}</p>
      {(['from', 'to'] as const).map(end => <Field key={end} label={end === 'from' ? 'Source agent' : 'Destination agent'} htmlFor={`${ids}-${end}`}>
        <select id={`${ids}-${end}`} className="fld-input" value={c[end]} disabled={locked}
          onChange={e => edit(d => reconnect(d, c.id, end, e.target.value))}>
          {draft.agents.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}
        </select>
      </Field>)}
      <Field label="Kind" htmlFor={`${ids}-kind`} reason={locked ? LOCK_REASON : null} reasonId={`${ids}-lock`}>
        <select
          id={`${ids}-kind`}
          className="fld-input"
          value={c.kind}
          disabled={locked}
          aria-describedby={locked ? `${ids}-lock` : undefined}
          onChange={(e) => edit((d) => setConnectionKind(d, c.id, e.target.value as ConnectionKind))}
        >
          <option value="hands-work-to">hands work to</option>
          <option value="reports-to">reports to ({NOT_ENFORCED})</option>
          <option value="watches">watches ({NOT_ENFORCED})</option>
        </select>
      </Field>
      <p className="fld-meta muted">“Hands work to” sets who starts first. The others are notes for now: saved, not enforced.</p>
      <div className="fld-actions">
        <Btn danger icon="trash" disabled={locked} onClick={() => {
          edit((d) => removeElement(d, { kind: 'connection', id: c.id }));
          onSelect(null);
        }}>
          Delete connection
        </Btn>
      </div>
    </SidePanel>
  );
}

/** Provenance: how the team was made, while it matters (e.g. drafted by AI). */
export function OriginLine({ origin, notRunYet }: { origin: TeamOrigin; notRunYet: boolean }) {
  const who = origin.cli === 'codex' ? 'Codex' : 'Claude Code';
  return (
    <div className="team-origin muted">
      <span>
        Generated by {who} from a description · {new Date(origin.at_ms).toLocaleDateString()}
        {notRunYet ? ' · not run yet' : ''}
      </span>
      <details>
        <summary>Description</summary>
        <p>{origin.prompt}</p>
      </details>
    </div>
  );
}
