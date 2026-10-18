import type { TeamOrigin } from '../generated/TeamOrigin';
import { ContractFields } from './ContractFields';
import { IntegrationFields, SkillFields } from './SkillFields';
import { SaveGroupButton } from '../groups/SaveGroupButton';
import { ProjectFolder } from './ProjectFolder';
import { effectiveFolder, folderForAll } from '../draft/folders';
import { TextArea } from '../shell/TextArea';
import { useEffect, useId, useRef, useState, type ReactNode, type RefObject } from 'react';
import type { ConnectionKind } from '../generated/ConnectionKind';
import type { DraftIssue } from '../generated/DraftIssue';
import type { ResumeBehaviour } from '../generated/ResumeBehaviour';
import type { TeamDraft } from '../generated/TeamDraft';
import { vendor } from '../herdr';
import {
  connect,
  reconnect,
  moveToGroup,
  removeElement,
  renameAgent,
  renameGroup,
  renameTeam,
  setAdvanced,
  setConnectionKind,
  setFolder,
  setJob,
  type Selection,
} from '../draft/edits';
import { drawnGroups } from '../draft/geometry';
import { groupOf, seatIdFor } from '../draft/ids';
import { issuesFor, MARK } from '../draft/issues';
import { Btn, Icon, VendorMark } from '../shell/ui';
import type { EditResult } from '../useTeamDraft';
import { CONNECTION_WORDS, KIND_LABEL, NOT_ENFORCED, WORDS } from '../vocab';
import { FolderPicker, shortPath, useHome } from './FolderPicker';
import { PickMenu } from './PickMenu';
import { AgentModelFields, GroupModelFields, TeamModelFields } from './ModelFields';
import { TokenLimitField } from './TokenLimitField';
import { plural } from '../format';

export type Edit = (fn: (d: TeamDraft) => EditResult, key?: string) => string | null;

/** Why structure can't change while the team runs. */
export const LOCK_REASON = 'Stop the team to change this.';

export const kindVendor = (k: 'claude' | 'codex' | 'practice') =>
  vendor(k === 'claude' ? 'claude-code' : k === 'codex' ? 'codex' : 'stub');

export const nameOf = (d: TeamDraft, id: string) => d.agents.find((a) => a.id === id)?.name.trim() || 'Unnamed agent';

const RESUME_WORDS: Record<ResumeBehaviour, string> = {
  'resume-if-possible': 'resume if possible',
  'relaunch-fresh': 'start fresh',
  'checkpoint-only': 'wait for me to choose',
};

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

/** A label above its control; `reason` explains a disabled control in visible text. */
function Field({ label, htmlFor, children, reason, reasonId }: { label: string; htmlFor?: string; children: ReactNode; reason?: string | null; reasonId?: string }) {
  return (
    <div className="fld">
      <label className="fld-label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {reason && (
        <p id={reasonId} className="reason fld-reason">
          {reason}
        </p>
      )}
    </div>
  );
}

type AgentPanelProps = {
  draft: TeamDraft;
  agentId: string;
  issues: DraftIssue[];
  edit: Edit;
  locked: boolean;
  onSelect: (s: Selection | null) => void;
  nameRef?: RefObject<HTMLInputElement | null>;
  /** Shown as a full panel (board) or as a plain section (narrow form editor). */
  bare?: boolean;
};

export function AgentPanel({ draft, agentId, issues, edit, locked, onSelect, nameRef, bare }: AgentPanelProps) {
  const ids = useId();
  const a = draft.agents.find((x) => x.id === agentId)!;
  const [menu, setMenu] = useState<'connect' | 'group' | null>(null);
  const [picking, setPicking] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const mine = issuesFor(issues, 'agent', a.id);
  const v = kindVendor(a.kind);
  const out = draft.connections.filter((c) => c.from === a.id);
  const incoming = draft.connections.filter((c) => c.to === a.id);
  const others = draft.agents.filter((x) => x.id !== a.id && !out.some((c) => c.to === x.id && c.kind === 'hands-work-to'));
  const groups = drawnGroups(draft).filter((g) => g.id !== groupOf(a));
  const inGroup = drawnGroups(draft).find((g) => g.id === groupOf(a));
  const connectReason = locked ? LOCK_REASON : draft.agents.length < 2 ? 'Add another agent first.' : null;
  const groupReason = locked ? LOCK_REASON : drawnGroups(draft).length === 0 ? 'Add a group first with + Group.' : groups.length === 0 ? 'There is no other group.' : null;
  const home = useHome();
  const folder = effectiveFolder(draft, a);
  const folderShown = folder ? shortPath(folder, home) : a.kind === 'practice' ? 'your home folder' : 'none chosen';
  const job = useRef<HTMLTextAreaElement>(null);

  useEffect(() => setConfirmDelete(false), [agentId]);

  const body = (
    <>
      <AgentModelFields draft={draft} edit={edit} locked={locked} a={a} />
      <Problems issues={mine} />
      <Field label="Name" htmlFor={`${ids}-name`}>
        <input
          id={`${ids}-name`}
          ref={nameRef}
          className="fld-input"
          value={a.name}
          spellCheck={false}
          onChange={(e) => edit((d) => renameAgent(d, a.id, e.target.value), `name:${a.id}`)}
        />
      </Field>
      <Field label="Job" htmlFor={`${ids}-job`}>
        <TextArea
          id={`${ids}-job`}
          ref={job}
          className="fld-input fld-job"
          value={a.job}
          rows={5}
          placeholder="What this agent should do."
          aria-describedby={`${ids}-jobhint`}
          onChange={(e) => edit((d) => setJob(d, a.id, e.target.value), `job:${a.id}`)}
        />
        <p id={`${ids}-jobhint`} className="fld-hint muted">
          {draft.connections.some((c) => c.kind === 'hands-work-to' && c.to === a.id && c.from !== a.id)
            ? 'Sent with its first handoff, as the Job reads then: you can still change it while this agent waits.'
            : 'Sent as its first message when the team starts. With no Job, your first message to this agent is its work: when it finishes, its result is handed over.'}
        </p>
      </Field>
      <Field label="Folder" reason={locked ? LOCK_REASON : null} reasonId={`${ids}-flock`}>
        <div className="fld-folder">
          <span className={`folder-path${a.folder ? '' : ' muted'}`} title={folder ?? undefined}>
            <Icon name="folder" size={14} className="muted" />
            <span className={a.folder ? 'mono' : undefined}>{folderShown}{!a.folder && draft.folder ? ' · from project' : ''}</span>
          </span>
          <span className="pane-spacer" />
          <Btn onClick={() => setPicking(true)} disabled={locked} aria-describedby={locked ? `${ids}-flock` : undefined} aria-label={`Choose a folder for ${a.name || 'this agent'}`} icon="folder" size="sm">
            Choose…
          </Btn>
          {a.folder && draft.agents.length > 1 && (
            <Btn onClick={() => edit((d) => folderForAll(d, a.folder!))} disabled={locked} aria-describedby={locked ? `${ids}-flock` : undefined}>
              Use this folder for every agent
            </Btn>
          )}
          {a.folder && (draft.folder || a.kind === 'practice') && (
            <Btn onClick={() => edit((d) => setFolder(d, a.id, null))} disabled={locked} aria-describedby={locked ? `${ids}-flock` : undefined}>
              {draft.folder ? 'Use project folder' : 'Clear'}
            </Btn>
          )}
        </div>
      </Field>
      <p className="fld-hint muted">Every message includes the configured team, jobs, folders and connections. Completed replies are handed to the connected agents automatically. “Reports to” and “watches” describe intent only.</p>
      <Field label="Hands work to" reason={connectReason} reasonId={`${ids}-creason`}>
        <ul className="conn-list">
          {out.map((c) => (
            <li key={c.id}>
              <button type="button" className="link" onClick={() => onSelect({ kind: 'connection', id: c.id })}>
                → {CONNECTION_WORDS[c.kind]} {nameOf(draft, c.to)}
                {c.kind !== 'hands-work-to' ? ` (${NOT_ENFORCED})` : ''}
              </button>
              {!locked && (
                <button
                  type="button"
                  className="x"
                  aria-label={`Remove: ${connectionSentence(draft, c)}`}
                  onClick={() => edit((d) => removeElement(d, { kind: 'connection', id: c.id }))}
                >
                  ×
                </button>
              )}
            </li>
          ))}
          {incoming.map((c) => (
            <li key={c.id} className="muted">
              <button type="button" className="link" onClick={() => onSelect({ kind: 'connection', id: c.id })}>
                ← {nameOf(draft, c.from)} {CONNECTION_WORDS[c.kind]} this agent
              </button>
            </li>
          ))}
        </ul>
        <div className="menu-anchor">
          <Btn onClick={() => setMenu(menu === 'connect' ? null : 'connect')} disabled={!!connectReason} aria-haspopup="menu" aria-expanded={menu === 'connect'} aria-describedby={connectReason ? `${ids}-creason` : undefined} icon="link" size="sm">
            Connect to…
          </Btn>
          {menu === 'connect' && (
            <PickMenu
              label={`${a.name || 'This agent'} hands work to`}
              options={others.map((o) => ({ id: o.id, label: o.name.trim() || 'Unnamed agent', note: KIND_LABEL[o.kind] }))}
              empty="It already hands work to every other agent."
              onClose={() => setMenu(null)}
              onPick={(to) => edit((d) => connect(d, a.id, to))}
            />
          )}
        </div>
      </Field>
      <Field label="Group" reason={groupReason} reasonId={`${ids}-greason`}>
        <p className="fld-value">{inGroup ? inGroup.label : 'no group'}</p>
        <div className="menu-anchor">
          <Btn onClick={() => setMenu(menu === 'group' ? null : 'group')} disabled={!!groupReason} aria-haspopup="menu" aria-expanded={menu === 'group'} aria-describedby={groupReason ? `${ids}-greason` : undefined} icon="group" size="sm">
            Move to group…
          </Btn>
          {menu === 'group' && (
            <PickMenu
              label={`Move ${a.name || 'this agent'} to`}
              options={groups.map((g) => ({ id: g.id, label: g.label.trim() || 'Unnamed group' }))}
              empty="There is no other group."
              onClose={() => setMenu(null)}
              onPick={(g) => edit((d) => moveToGroup(d, a.id, g))}
            />
          )}
        </div>
      </Field>
      <TokenLimitField value={a.advanced.token_limit} turns={a.advanced.turn_limit} minutes={a.advanced.minute_limit} disabled={locked} reason={locked ? LOCK_REASON : null}
        onChange={(patch) => edit((d) => setAdvanced(d, a.id, patch))} />
      <Field label="Check" htmlFor={`${ids}-check`} reason={locked ? LOCK_REASON : null}>
        <input
          id={`${ids}-check`}
          className="fld-input mono"
          placeholder="e.g. npm test"
          value={a.advanced.check ?? ''}
          disabled={locked}
          maxLength={1000}
          onChange={(e) => edit((d) => setAdvanced(d, a.id, { check: e.target.value.trim() ? e.target.value : undefined }))}
        />
        <p className="fld-hint muted">A command Sculptured runs in this agent's folder after each turn it finishes. Exit 0 shows “check passed”; anything else “check failed”, with its output. Runs as you, with your permissions.</p>
      </Field>
      <SkillFields agent={a} edit={edit} locked={locked} />
      <details className="fld-advanced" data-advanced>
        <summary>{WORDS.advanced}</summary>
        <Field label="When it restarts" htmlFor={`${ids}-resume`}>
          <select
            id={`${ids}-resume`}
            className="fld-input"
            value={a.advanced.resume}
            disabled={locked}
            onChange={(e) => edit((d) => setAdvanced(d, a.id, { resume: e.target.value as ResumeBehaviour }))}
          >
            {(Object.keys(RESUME_WORDS) as ResumeBehaviour[]).map((r) => (
              <option key={r} value={r}>
                {RESUME_WORDS[r]}
              </option>
            ))}
          </select>
        </Field>
        <Field label="Permissions" htmlFor={`${ids}-perm`}>
          <select
            id={`${ids}-perm`}
            className="fld-input"
            value={a.advanced.permission}
            disabled={locked}
            onChange={(e) => edit((d) => setAdvanced(d, a.id, { permission: e.target.value as 'restricted' | 'unrestricted' }))}
          >
            <option value="unrestricted">unrestricted</option>
            <option value="restricted">restricted</option>
          </select>
        </Field>
        <ContractFields draft={draft} agent={a} edit={edit} locked={locked} />
        <IntegrationFields agent={a} edit={edit} locked={locked} draft={draft} />
        <p className="fld-meta">
          {WORDS.internalId}: <code>{a.id}</code> · address <code>{seatIdFor(draft, a.id)}</code>
        </p>
      </details>
      <div className="fld-actions">
        {confirmDelete ? (
          <div role="group" aria-label="Confirm delete agent" className="confirm">
            <p>
              Deletes {a.name || 'this agent'}
              {out.length + incoming.length > 0 ? ` and its ${plural(out.length + incoming.length, 'connection')}` : ''}. You can undo this.
            </p>
            <Btn
              danger
              onClick={() => {
                edit((d) => removeElement(d, { kind: 'agent', id: a.id }));
                onSelect(null);
              }}
            >
              Delete agent
            </Btn>
            <Btn onClick={() => setConfirmDelete(false)}>Cancel</Btn>
          </div>
        ) : (
          <>
            <Btn danger icon="trash" onClick={() => setConfirmDelete(true)} disabled={locked} aria-describedby={locked ? `${ids}-dlock` : undefined}>
              Delete agent…
            </Btn>
            {locked && (
              <span id={`${ids}-dlock`} className="reason">
                {LOCK_REASON}
              </span>
            )}
          </>
        )}
      </div>
      {picking && (
        <FolderPicker
          initialPath={folder}
          onClose={() => setPicking(false)}
          onChoose={(path) => {
            setPicking(false);
            edit((d) => setFolder(d, a.id, path));
          }}
        />
      )}
    </>
  );
  if (bare) return <div className="agent-form">{body}</div>;
  return (
    <SidePanel
      label={`Agent ${a.name || 'Unnamed agent'}`}
      title={
        <>
          <VendorMark v={v} size="sm" /> <span className="eyebrow">Agent</span> <span className={`team-title ${v.cls}`}>{a.name || 'Unnamed agent'}</span>
        </>
      }
      tools={
        <Btn variant="ghost" icon="x" iconOnly onClick={() => onSelect(null)} aria-label="Close agent panel" kbd="esc" />
      }
    >
      {body}
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
function OriginLine({ origin, notRunYet }: { origin: TeamOrigin; notRunYet: boolean }) {
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

export function TeamPanel({ draft, edit, children, locked = false, origin = null, notRunYet = false }: { draft: TeamDraft; edit: Edit; children?: ReactNode; locked?: boolean; origin?: TeamOrigin | null; notRunYet?: boolean }) {
  const ids = useId();
  const groups = drawnGroups(draft).length;
  return (
    <SidePanel label="Team" title="Team">
      {origin && <OriginLine origin={origin} notRunYet={notRunYet} />}
      <Field label="Team name" htmlFor={`${ids}-name`}>
        <input id={`${ids}-name`} className="fld-input" value={draft.name} spellCheck={false} onChange={(e) => edit((d) => renameTeam(d, e.target.value), 'team-name')} />
      </Field>
      <p className="fld-value muted">
        {plural(draft.agents.length, 'agent')} · {plural(draft.connections.length, 'connection')} · {plural(groups, 'group')}
      </p>
      <ProjectFolder draft={draft} edit={edit} locked={locked} />
      <TeamModelFields draft={draft} edit={edit} locked={locked} />
      <dl className="how-to">
        <dt>add</dt>
        <dd>
          drag <span className="kbdish">+ Claude</span> onto the board, or press <kbd>n</kbd> then <kbd>⏎</kbd>
        </dd>
        <dt>connect</dt>
        <dd>
          drag from an agent’s <span className="port-glyph">●</span> to another agent, or <span className="kbdish">Connect to…</span>
        </dd>
        <dt>move</dt>
        <dd>
          drag a card, or select it and press <kbd>←↑→↓</kbd> (<kbd>⇧</kbd> for bigger steps)
        </dd>
        <dt>edit</dt>
        <dd>
          click an agent, or <kbd>⏎</kbd> on it, to set its name, job and folder
        </dd>
        <dt>undo</dt>
        <dd>
          <kbd>Ctrl+Z</kbd> · redo <kbd>Ctrl+⇧+Z</kbd> · delete <kbd>Del</kbd>
        </dd>
      </dl>
      {children}
    </SidePanel>
  );
}
