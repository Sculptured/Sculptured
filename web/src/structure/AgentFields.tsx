import { useId, useState, type ReactNode, type RefObject } from 'react';
import { connect, moveToGroup, removeElement, renameAgent, setAdvanced, setConnectionKind, setFolder, setJob } from '../draft/edits';
import { effectiveFolder, folderForAll } from '../draft/folders';
import { drawnGroups } from '../draft/geometry';
import { groupOf, seatIdFor } from '../draft/ids';
import { MARK } from '../draft/issues';
import { groupDefault, isModelKind, setKind, setRunsOn, usesConnections, type ModelKind } from '../draft/models';
import { Link } from 'react-router-dom';
import { useConnections, type ModelConnection } from '../hosted/connections';
import type { ConnectionKind } from '../generated/ConnectionKind';
import type { DraftAgent } from '../generated/DraftAgent';
import type { DraftIssue } from '../generated/DraftIssue';
import type { ResumeBehaviour } from '../generated/ResumeBehaviour';
import type { TeamDraft } from '../generated/TeamDraft';
import { plural } from '../format';
import { TextArea } from '../shell/TextArea';
import { Btn, Icon } from '../shell/ui';
import { KIND_LABEL, NOT_ENFORCED, WORDS } from '../vocab';
import { BriefingField } from './BriefingField';
import { StatusOwnerField } from './StatusOwnerField';
import { ContractFields } from './ContractFields';
import { FolderPicker, shortPath, useHome } from './FolderPicker';
import { KindSwitch, modelLabel, ModelSelect } from './ModelControls';
import { LOCK_REASON, nameOf, type Edit } from './Panels';
import { PickMenu } from './PickMenu';
import { RunsOnPicker } from './RunsOnPicker';
import { IntegrationFields, SkillFields } from './SkillFields';
import { TokenLimitField } from './TokenLimitField';
import { useHosted } from '../hosted/HostedGate';
import { AgentRunOn, useAgentHosts } from '../hosted/RunOn';

type Common = { draft: TeamDraft; a: DraftAgent; edit: Edit; locked: boolean };

/** Which field a problem is about, so it shows beside that field. */
export type FieldKey = 'name' | 'folder' | 'connections' | 'advanced' | 'other';
export function fieldOf(i: DraftIssue): FieldKey {
  if (i.code.startsWith('agent-name')) return 'name';
  if (i.code.startsWith('folder-') || i.code === 'home-unknown') return 'folder';
  if (i.element.kind === 'connection' || i.code.startsWith('connection-')) return 'connections';
  if (i.code === 'skill-invalid' || i.code === 'safety-rule-invalid' || i.code === 'agent-group-missing') return 'advanced';
  return 'other';
}

/** One compact problem line under a field: shape plus words. */
export function FieldIssues({ issues }: { issues: DraftIssue[] }) {
  if (issues.length === 0) return null;
  return (
    <p className="fld-issue">
      {issues.map((i, n) => (
        <span key={`${i.code}-${n}`} className={i.severity === 'error' ? 'tone-red' : 'tone-amber'}><span aria-hidden="true">{MARK[i.severity]} </span>{i.message}</span>
      ))}
    </p>
  );
}

/** A short "?" that explains on hover and focus, instead of a paragraph. `id` holds the words, for aria-describedby. */
export function Hint({ text, id }: { text: string; id?: string }) {
  return (
    <>
      <span className="fld-help" role="img" tabIndex={0} aria-label={text} data-tip={text}><Icon name="help" size={13} /></span>
      {id && <span id={id} className="sr-only">{text}</span>}
    </>
  );
}

/** A label above its control; `help` is a "?" beside it (its words in `helpId`); `reason` explains a disabled control in visible text. */
export function Field({ label, htmlFor, children, reason, reasonId, help, helpId, tour }: { label: string; htmlFor?: string; children: ReactNode; reason?: string | null; reasonId?: string; help?: string; helpId?: string; /** An anchor for the setup tour. */ tour?: string }) {
  return (
    <div className="fld" data-tour={tour}>
      <div className="fld-label-row"><label className="fld-label" htmlFor={htmlFor}>{label}</label>{help && <Hint text={help} id={helpId} />}</div>
      {children}
      {reason && <p id={reasonId} className="reason fld-reason">{reason}</p>}
    </div>
  );
}

export function NameField({ a, edit, inputRef, issues = [] }: { a: DraftAgent; edit: Edit; inputRef?: RefObject<HTMLInputElement | null>; issues?: DraftIssue[] }) {
  const ids = useId();
  return (
    <Field label="Name" htmlFor={`${ids}-name`}>
      <input id={`${ids}-name`} ref={inputRef} className="fld-input" value={a.name} spellCheck={false} onChange={(e) => edit((d) => renameAgent(d, a.id, e.target.value), `name:${a.id}`)} />
      <FieldIssues issues={issues} />
    </Field>
  );
}

/** "Runs on" (Claude, Codex or OpenCode, and hosted, your model connections) and its model, side by side. */
export function RunsOnField({ draft, a, edit, locked }: Common) {
  const k = a.kind;
  const hosted = useHosted().kind === 'signed-in';
  const conns = useConnections(hosted);
  const conn = usesConnections(k) && a.connection ? conns.list.find((c) => c.id === a.connection) ?? null : null;
  const picker = hosted && (conns.list.length > 0 || (conns.loaded && !!a.connection));
  const fallback = isModelKind(k) ? groupDefault(draft, a.group_id, k) : null;
  const from = a.group_id && draft.groups.find((g) => g.id === a.group_id)?.models?.[k as ModelKind] ? 'group' : 'team';
  const label = `${a.name || 'This agent'} runs on`;
  return (
    <div className="fld runs-on">
      <span className="fld-label">Runs on</span>
      <div className="runs-on-row">
        {picker
          ? <RunsOnPicker a={a} label={label} conns={conns.list} disabled={locked} onChange={(kind, id) => edit((d) => setRunsOn(d, a.id, kind, id))} />
          : <KindSwitch label={label} value={k} disabled={locked} onChange={(next) => edit((d) => setKind(d, a.id, next))} />}
        {conn?.provider === 'openrouter' ? (
          <span className="muted">Model: <code>{conn.model}</code> · from the connection</span>
        ) : isModelKind(k) ? (
          <ModelSelect key={k} kind={k} label={`${KIND_LABEL[k]} model for ${a.name || 'this agent'}`} value={a.advanced.model}
            inherited={fallback ? `${modelLabel(k, fallback)}, from the ${from}` : modelLabel(k, null)} disabled={locked}
            onChange={(model) => edit((d) => setAdvanced(d, a.id, { model }), `model:${a.id}`)} />
        ) : (
          <span className="muted">free · no model calls</span>
        )}
      </div>
      <ConnectionWarning a={a} conn={conn} loaded={conns.loaded} />
      {locked && <p className="reason fld-reason">{LOCK_REASON}</p>}
    </div>
  );
}

/** A chosen connection whose last check failed, or that was removed. */
function ConnectionWarning({ a, conn, loaded }: { a: DraftAgent; conn: ModelConnection | null; loaded: boolean }) {
  if (conn?.checked?.ok === false) return <p className="fld-issue tone-amber"><span aria-hidden="true">⚠ </span>This connection’s last check failed. <Link to="/setup?step=models">Check it in Setup</Link></p>;
  if (usesConnections(a.kind) && a.connection && loaded && !conn) return <p className="fld-issue tone-amber"><span aria-hidden="true">⚠ </span>This agent’s model connection was removed. Choose another.</p>;
  return null;
}

const JOB_HELP_FIRST = 'Sent as its first message when the team starts. With no Job, your first message to it is its work.';
const JOB_HELP_HANDOFF = 'Sent with its first handoff, as it reads then: you can still change it while this agent waits.';

export function JobField({ draft, a, edit, rows = 6 }: { draft: TeamDraft; a: DraftAgent; edit: Edit; rows?: number }) {
  const ids = useId();
  const handedTo = draft.connections.some((c) => c.kind === 'hands-work-to' && c.to === a.id && c.from !== a.id);
  return (
    <Field label="Job" htmlFor={`${ids}-job`} help={handedTo ? JOB_HELP_HANDOFF : JOB_HELP_FIRST} helpId={`${ids}-jobhint`}>
      <TextArea id={`${ids}-job`} aria-describedby={`${ids}-jobhint`} className="fld-input fld-job" value={a.job} rows={rows} placeholder="What this agent should do." onChange={(e) => edit((d) => setJob(d, a.id, e.target.value), `job:${a.id}`)} />
    </Field>
  );
}

export function FolderField({ draft, a, edit, locked, issues = [] }: Common & { issues?: DraftIssue[] }) {
  const ids = useId();
  const [picking, setPicking] = useState(false);
  const home = useHome();
  const hosted = useHosted().kind === 'signed-in';
  // An agent on its own computer never uses the project folder: that path is on the team's computer.
  const own = useAgentHosts(draft.id, hosted)[a.id];
  const folder = own ? a.folder?.trim() || null : effectiveFolder(draft, a);
  const shown = folder ? shortPath(folder, home) : own ? `${own.name}’s own folder` : a.kind === 'practice' ? 'your home folder' : hosted ? 'its computer’s own folder' : 'none chosen';
  const off = { disabled: locked, 'aria-describedby': locked ? `${ids}-lock` : undefined };
  return (<>
    <HostedRunOn draft={draft} a={a} />
    <Field tour="agent-folder" label={own ? `Folder on ${own.name}` : 'Folder'} reason={locked ? LOCK_REASON : null} reasonId={`${ids}-lock`}>
      <div className="fld-folder">
        <span className={`folder-path${a.folder ? '' : ' muted'}`} title={folder ?? undefined}>
          <Icon name="folder" size={14} className="muted" />
          <span className={a.folder ? 'mono' : undefined}>{shown}{!a.folder && !own && draft.folder ? ' · from project' : ''}</span>
        </span>
        <Btn {...off} onClick={() => setPicking(true)} aria-label={`Choose a folder for ${a.name || 'this agent'}`} icon="folder" size="sm">Choose…</Btn>
        {a.folder && !own && draft.agents.length > 1 && <Btn {...off} size="sm" variant="ghost" onClick={() => edit((d) => folderForAll(d, a.folder!))}>Use this folder for every agent</Btn>}
        {a.folder && (own || draft.folder || a.kind === 'practice') && <Btn {...off} size="sm" variant="ghost" onClick={() => edit((d) => setFolder(d, a.id, null))}>{own ? `Use ${own.name}’s own folder` : draft.folder ? 'Use project folder' : 'Clear'}</Btn>}
      </div>
      <FieldIssues issues={issues} />
      {picking && <FolderPicker team={hosted ? draft.id : undefined} host={own?.id} initialPath={folder} onClose={() => setPicking(false)} onChoose={(path) => { setPicking(false); edit((d) => setFolder(d, a.id, path)); }} />}
    </Field>
  </>);
}

/** Who this agent hands work to (and other links), who hands it work, and Connect to… */
export function ConnectionsField({ draft, a, edit, locked, issues = [] }: Common & { issues?: DraftIssue[] }) {
  const ids = useId();
  const [menu, setMenu] = useState(false);
  const out = draft.connections.filter((c) => c.from === a.id);
  const incoming = draft.connections.filter((c) => c.to === a.id);
  const others = draft.agents.filter((x) => x.id !== a.id && !out.some((c) => c.to === x.id && c.kind === 'hands-work-to'));
  const reason = locked ? LOCK_REASON : draft.agents.length < 2 ? 'Add another agent first.' : null;
  return (
    <Field label="Connections" reason={reason} reasonId={`${ids}-why`} help="“Hands work to” passes this agent's finished reply on. “Reports to” and “watches” are notes only.">
      <ul className="conn-list conn-rows">
        {out.map((c) => (
          <li key={c.id}>
            <select className="fld-input conn-kind" aria-label={`How ${a.name || 'this agent'} is linked to ${nameOf(draft, c.to)}`} value={c.kind} disabled={locked}
              onChange={(e) => edit((d) => setConnectionKind(d, c.id, e.target.value as ConnectionKind))}>
              <option value="hands-work-to">hands work to</option>
              <option value="reports-to">reports to ({NOT_ENFORCED})</option>
              <option value="watches">watches ({NOT_ENFORCED})</option>
            </select>
            <span className="conn-who">{nameOf(draft, c.to)}</span>
            {!locked && <button type="button" className="x" aria-label={`Remove: ${a.name || 'this agent'} → ${nameOf(draft, c.to)}`} onClick={() => edit((d) => removeElement(d, { kind: 'connection', id: c.id }))}>×</button>}
          </li>
        ))}
        {incoming.map((c) => <li key={c.id} className="muted">← {nameOf(draft, c.from)} {c.kind === 'hands-work-to' ? 'hands work to this agent' : `${c.kind === 'reports-to' ? 'reports to' : 'watches'} this agent`}</li>)}
        {out.length + incoming.length === 0 && <li className="muted">Not connected to any agent.</li>}
      </ul>
      <div className="menu-anchor">
        <Btn onClick={() => setMenu(!menu)} disabled={!!reason} aria-haspopup="menu" aria-expanded={menu} aria-describedby={reason ? `${ids}-why` : undefined} icon="link" size="sm">Connect to…</Btn>
        {menu && (
          <PickMenu label={`${a.name || 'This agent'} hands work to`} options={others.map((o) => ({ id: o.id, label: o.name.trim() || 'Unnamed agent', note: KIND_LABEL[o.kind] }))}
            empty="It already hands work to every other agent." onClose={() => setMenu(false)} onPick={(to) => edit((d) => connect(d, a.id, to))} />
        )}
      </div>
      <FieldIssues issues={issues} />
    </Field>
  );
}

export function GroupField({ draft, a, edit, locked }: Common) {
  const ids = useId();
  const [menu, setMenu] = useState(false);
  const all = drawnGroups(draft);
  const groups = all.filter((g) => g.id !== groupOf(a));
  const inGroup = all.find((g) => g.id === groupOf(a));
  const reason = locked ? LOCK_REASON : all.length === 0 ? 'Add a group on the board first (More tools · Group).' : groups.length === 0 ? 'There is no other group.' : null;
  return (
    <Field label="Group" reason={reason} reasonId={`${ids}-why`}>
      <div className="fld-inline">
        <span className="fld-value">{inGroup ? inGroup.label : 'no group'}</span>
        <span className="menu-anchor">
          <Btn onClick={() => setMenu(!menu)} disabled={!!reason} aria-haspopup="menu" aria-expanded={menu} aria-describedby={reason ? `${ids}-why` : undefined} icon="group" size="sm">Move to group…</Btn>
          {menu && <PickMenu label={`Move ${a.name || 'this agent'} to`} options={groups.map((g) => ({ id: g.id, label: g.label.trim() || 'Unnamed group' }))} empty="There is no other group." onClose={() => setMenu(false)} onPick={(g) => edit((d) => moveToGroup(d, a.id, g))} />}
        </span>
      </div>
    </Field>
  );
}

export function LimitsField({ a, edit, locked }: Common) {
  return <TokenLimitField value={a.advanced.token_limit} turns={a.advanced.turn_limit} minutes={a.advanced.minute_limit} disabled={locked} reason={locked ? LOCK_REASON : null} onChange={(patch) => edit((d) => setAdvanced(d, a.id, patch))} />;
}

export function CheckField({ a, edit, locked }: Common) {
  const ids = useId();
  return (
    <Field label="Check" htmlFor={`${ids}-check`} reason={locked ? LOCK_REASON : null} help="A command run in this agent's folder after each turn it finishes. Exit 0 shows “check passed”. Runs as you." helpId={`${ids}-checkhint`}>
      <input id={`${ids}-check`} aria-describedby={`${ids}-checkhint`} className="fld-input mono" placeholder="e.g. npm test" value={a.advanced.check ?? ''} disabled={locked} maxLength={1000}
        onChange={(e) => edit((d) => setAdvanced(d, a.id, { check: e.target.value.trim() ? e.target.value : undefined }))} />
    </Field>
  );
}

const RESUME_WORDS: Record<ResumeBehaviour, string> = { 'resume-if-possible': 'resume if possible', 'relaunch-fresh': 'start fresh', 'checkpoint-only': 'wait for me to choose' };

/** Restart behaviour, permissions, contract, integrations and ids: the raw configuration (`data-advanced`). */
export function RawConfig({ draft, a, edit, locked }: Common) {
  const ids = useId();
  return (
    <details className="fld-advanced" data-advanced>
      <summary>{WORDS.advanced}</summary>
      <Field label="When it restarts" htmlFor={`${ids}-resume`}>
        <select id={`${ids}-resume`} className="fld-input" value={a.advanced.resume} disabled={locked} onChange={(e) => edit((d) => setAdvanced(d, a.id, { resume: e.target.value as ResumeBehaviour }))}>
          {(Object.keys(RESUME_WORDS) as ResumeBehaviour[]).map((r) => <option key={r} value={r}>{RESUME_WORDS[r]}</option>)}
        </select>
      </Field>
      <Field label="Permissions" htmlFor={`${ids}-perm`}>
        <select id={`${ids}-perm`} className="fld-input" value={a.advanced.permission} disabled={locked} onChange={(e) => edit((d) => setAdvanced(d, a.id, { permission: e.target.value as 'restricted' | 'unrestricted' }))}>
          <option value="unrestricted">unrestricted</option>
          <option value="restricted">restricted</option>
        </select>
      </Field>
      <ContractFields draft={draft} agent={a} edit={edit} locked={locked} />
      <IntegrationFields agent={a} edit={edit} locked={locked} draft={draft} />
      <p className="fld-meta">{WORDS.internalId}: <code>{a.id}</code> · address <code>{seatIdFor(draft, a.id)}</code></p>
    </details>
  );
}

/** Hosted: which of your computers this agent runs on (the team's unless chosen). */
function HostedRunOn({ draft, a }: { draft: TeamDraft; a: DraftAgent }) {
  if (useHosted().kind !== 'signed-in') return null;
  return <AgentRunOn team={draft.id} agent={a.id} name={a.name || 'This agent'} />;
}

/** Group, limits, check, briefing, skills and the raw configuration. */
export function AdvancedFields(p: Common) {
  return (
    <>
      <GroupField {...p} />
      <LimitsField {...p} />
      <CheckField {...p} />
      <BriefingField a={p.a} edit={p.edit} locked={p.locked} />
      <StatusOwnerField {...p} />
      <SkillFields agent={p.a} edit={p.edit} locked={p.locked} />
      <RawConfig {...p} />
    </>
  );
}

/** Delete agent…, asked once more; Ctrl+Z brings it back. */
export function DeleteAgent({ draft, a, edit, locked, onDeleted }: Common & { onDeleted: () => void }) {
  const ids = useId();
  const [asking, setAsking] = useState(false);
  const links = draft.connections.filter((c) => c.from === a.id || c.to === a.id).length;
  if (asking)
    return (
      <div role="group" aria-label="Confirm delete agent" className="confirm">
        <p>Deletes {a.name || 'this agent'}{links > 0 ? ` and its ${plural(links, 'connection')}` : ''}. You can undo this.</p>
        <Btn danger onClick={() => { edit((d) => removeElement(d, { kind: 'agent', id: a.id })); onDeleted(); }}>Delete agent</Btn>
        <Btn onClick={() => setAsking(false)}>Cancel</Btn>
      </div>
    );
  return (
    <span className="fld-actions">
      <Btn danger icon="trash" onClick={() => setAsking(true)} disabled={locked} aria-describedby={locked ? `${ids}-lock` : undefined}>Delete agent…</Btn>
      {locked && <span id={`${ids}-lock`} className="reason">{LOCK_REASON}</span>}
    </span>
  );
}
