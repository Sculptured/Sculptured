import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';
import { setAdvanced } from '../draft/edits';
import type { SharedValue } from '../generated/SharedValue';

/** `name = value @ file`; null when a part is missing. */
export function parseShared(line: string): SharedValue | null {
  const eq = line.indexOf('='), at = line.lastIndexOf('@');
  if (eq < 1 || at <= eq) return null;
  const v = { name: line.slice(0, eq).trim(), value: line.slice(eq + 1, at).trim(), file: line.slice(at + 1).trim() };
  return v.name && v.value && v.file ? v : null;
}
import type { Edit } from './Panels';

const lines = (text: string) => text.split('\n').map((l) => l.trim()).filter(Boolean);

type DataKey = 'reads' | 'writes' | 'side_effects';
const DATA: { key: DataKey; label: string; hint: string }[] = [
  { key: 'reads', label: 'Reads', hint: 'Files and folders it reads, one per line, relative to its folder. Empty: anything.' },
  { key: 'writes', label: 'Writes', hint: 'Files and folders it changes. Protections refuse its Read, Write and Edit calls outside these; shell commands are not checked.' },
  { key: 'side_effects', label: 'Other effects', hint: 'Effects beyond its files, in plain words: “sends email”, “pushes to main”.' },
];

/** Values the agent shares with others, checked in their files at every start (an idea from Constrain). */
function SharedField({ agent, edit, locked }: { agent: DraftAgent; edit: Edit; locked: boolean }) {
  const data = agent.advanced.data_access ?? {};
  const id = (k: string) => `contract-${agent.id}-${k}`;
  const setShared = (text: string) => edit((d: TeamDraft) => {
    const shared = lines(text).map(parseShared).filter((v): v is SharedValue => v !== null);
    const next = { ...data, shared };
    const empty = !next.reads?.length && !next.writes?.length && !next.side_effects?.length && !next.rationale && !shared.length;
    return setAdvanced(d, agent.id, { data_access: empty ? undefined : next });
  }, `contract:${agent.id}:shared`);
  return (
    <label htmlFor={id('shared')} className="fld">
      <span>Values it shares</span>
      <textarea id={id('shared')} className="fld-input mono" rows={2} disabled={locked} placeholder="api port = 8080 @ server/config.py" defaultValue={(data.shared ?? []).map((v) => `${v.name} = ${v.value} @ ${v.file}`).join('\n')} onBlur={(e) => setShared(e.target.value)} />
      <span className="fld-hint muted">Values other agents rely on, one per line as <code>name = exact value @ file</code>. Every start checks each file still contains its value, and agents are told what was found.</span>
    </label>
  );
}

type FieldProps = { agent: DraftAgent; edit: Edit; locked: boolean };

const fieldId = (agent: DraftAgent, k: string) => `contract-${agent.id}-${k}`;

function DecidesField({ agent, edit, locked }: FieldProps) {
  const setDomains = (text: string) => edit((d: TeamDraft) => {
    const domains = lines(text);
    return setAdvanced(d, agent.id, { authority: domains.length ? { ...agent.advanced.authority, domains } : undefined });
  }, `contract:${agent.id}:domains`);
  return (
    <label htmlFor={fieldId(agent, 'domains')} className="fld">
      <span>Decides</span>
      <textarea id={fieldId(agent, 'domains')} className="fld-input" rows={2} disabled={locked} value={(agent.advanced.authority?.domains ?? []).join('\n')} onChange={(e) => setDomains(e.target.value)} />
      <span className="fld-hint muted">Decisions this agent owns, one per line. Every agent is told who decides what, and asks the owner first.</span>
    </label>
  );
}

function ScopeField({ draft, agent, edit, locked }: FieldProps & { draft: TeamDraft }) {
  const setScope = (scope: string) => edit((d: TeamDraft) => setAdvanced(d, agent.id, { authority: { domains: agent.advanced.authority?.domains ?? [], ...agent.advanced.authority, scope: scope || undefined } }), `contract:${agent.id}:scope`);
  return (
    <label htmlFor={fieldId(agent, 'scope')} className="fld">
      <span>Its decisions reach</span>
      <select id={fieldId(agent, 'scope')} className="fld-input" disabled={locked || !agent.advanced.authority?.domains.length} value={agent.advanced.authority?.scope ?? ''} onChange={(e) => setScope(e.target.value)}>
        <option value="">its own work</option>
        {draft.groups.map((g) => <option key={g.id} value={g.id}>across the {g.label.trim() || 'unnamed'} group</option>)}
        <option value="system">across the whole team</option>
      </select>
      <span className="fld-hint muted">A decision belongs to the agent whose reach contains its consequences: others take cross-group decisions to it instead of agreeing them between themselves.</span>
    </label>
  );
}

const isEmptyAccess = (a: NonNullable<DraftAgent['advanced']['data_access']>) => !a.reads?.length && !a.writes?.length && !a.side_effects?.length && !a.rationale && !a.shared?.length;

/** The agent's contract (MADL `data_access` and `authority`): what it touches and what it decides. */
export function ContractFields({ draft, agent, edit, locked }: { draft: TeamDraft; agent: DraftAgent; edit: Edit; locked: boolean }) {
  const data = agent.advanced.data_access ?? {};
  const setData = (key: DataKey, text: string) => edit((d: TeamDraft) => {
    const next = { ...data, [key]: lines(text) };
    return setAdvanced(d, agent.id, { data_access: isEmptyAccess(next) ? undefined : next });
  }, `contract:${agent.id}:${key}`);
  return (
    <fieldset className="fld-contract">
      <legend className="fld-label">Contract</legend>
      {DATA.map((f) => (
        <label key={f.key} htmlFor={fieldId(agent, f.key)} className="fld">
          <span>{f.label}</span>
          <textarea id={fieldId(agent, f.key)} className="fld-input mono" rows={2} disabled={locked} value={(data[f.key] ?? []).join('\n')} onChange={(e) => setData(f.key, e.target.value)} />
          <span className="fld-hint muted">{f.hint}</span>
        </label>
      ))}
      <SharedField agent={agent} edit={edit} locked={locked} />
      <DecidesField agent={agent} edit={edit} locked={locked} />
      <ScopeField draft={draft} agent={agent} edit={edit} locked={locked} />
    </fieldset>
  );
}
