import { Btn } from '../shell/ui';
import './agent-details.css';
import type { DraftAgent } from '../generated/DraftAgent';
import type { ModelDefaults } from '../generated/ModelDefaults';
import type { TeamDraft } from '../generated/TeamDraft';
import { setAdvanced } from '../draft/edits';
import { groupDefault, isModelKind, notOnTeamModel, setGroupModel, setKind, setKindAll, setTeamModel, toTeamModels, type ModelKind } from '../draft/models';
import { KIND_LABEL } from '../vocab';
import { KindSwitch, modelLabel, ModelSelect } from './ModelControls';
import { LOCK_REASON, type Edit } from './Panels';

type Common = { draft: TeamDraft; edit: Edit; locked: boolean };

function LockNote({ locked }: { locked: boolean }) {
  return locked ? <p className="reason fld-reason">{LOCK_REASON}</p> : null;
}

/** "Runs on" and "Model" for one agent, at the top of its panel. */
export function AgentModelFields({ draft, edit, locked, a }: Common & { a: DraftAgent }) {
  const k = a.kind;
  const fallback = isModelKind(k) ? groupDefault(draft, a.group_id, k) : null;
  const from = a.group_id && draft.groups.find((g) => g.id === a.group_id)?.models?.[k as ModelKind] ? 'group' : 'team';
  return (
    <div className="fld model-fields">
      <span className="fld-label">Runs on</span>
      <KindSwitch label={`${a.name || 'This agent'} runs on`} value={k} disabled={locked} onChange={(next) => edit((d) => setKind(d, a.id, next))} />
      {isModelKind(k) ? (
        <ModelSelect
          key={k}
          kind={k}
          label={`${KIND_LABEL[k]} model for ${a.name || 'this agent'}`}
          value={a.advanced.model}
          inherited={fallback ? `${modelLabel(k, fallback)}, from the ${from}` : modelLabel(k, null)}
          disabled={locked}
          onChange={(model) => edit((d) => setAdvanced(d, a.id, { model }), `model:${a.id}`)}
        />
      ) : (
        <p className="muted fld-hint">Practice agents are free and make no model calls.</p>
      )}
      <LockNote locked={locked} />
    </div>
  );
}

/** A Claude, a Codex and an OpenCode model, for a team or a group. */
function DefaultsPair({ models, inherited, label, locked, onChange }: { models?: ModelDefaults; inherited: (k: ModelKind) => string; label: string; locked: boolean; onChange: (k: ModelKind, m: string | null) => void }) {
  return (
    <>
      {(['claude', 'codex', 'opencode'] as ModelKind[]).map((k) => (
        <label key={k} className="model-row">
          <span className="model-row-kind">{KIND_LABEL[k]} agents</span>
          <ModelSelect kind={k} label={`${label}: ${KIND_LABEL[k]} model`} value={models?.[k] ?? null} inherited={inherited(k)} disabled={locked} onChange={(m) => onChange(k, m)} />
        </label>
      ))}
    </>
  );
}

const kindOfAll = (agents: DraftAgent[]) => {
  const kinds = new Set(agents.map((a) => a.kind));
  return kinds.size === 1 ? [...kinds][0] : null;
};

/** Team-wide: switch every agent at once, and the team's model for each kind. */
export function TeamModelFields({ draft, edit, locked }: Common) {
  return (
    <div className="fld model-fields">
      <span className="fld-label">All agents run on</span>
      <KindSwitch label="All agents run on" value={kindOfAll(draft.agents)} disabled={locked || draft.agents.length === 0} onChange={(k) => edit((d) => setKindAll(d, k))} />
      <p className="muted fld-hint">Switches every agent. Ctrl+Z undoes it.</p>
      <span className="fld-label">Team models</span>
      <DefaultsPair models={draft.models} label="Team" locked={locked} inherited={(k) => modelLabel(k, null)} onChange={(k, m) => edit((d) => setTeamModel(d, k, m), `team-model:${k}`)} />
      <OwnModels draft={draft} edit={edit} locked={locked} />
      <LockNote locked={locked} />
    </div>
  );
}

/** Group-wide: switch the group's agents, and the group's model for each kind. */
export function GroupModelFields({ draft, edit, locked, groupId }: Common & { groupId: string }) {
  const g = draft.groups.find((x) => x.id === groupId)!;
  const members = draft.agents.filter((a) => a.group_id === groupId);
  const teamModel = (k: ModelKind) => draft.models?.[k] ?? null;
  return (
    <div className="fld model-fields">
      <span className="fld-label">Agents in this group run on</span>
      <KindSwitch label={`${g.label || 'Group'} runs on`} value={kindOfAll(members)} disabled={locked || members.length === 0} onChange={(k) => edit((d) => setKindAll(d, k, members.map((a) => a.id)))} />
      <span className="fld-label">Group models</span>
      <DefaultsPair models={g.models} label={g.label || 'Group'} locked={locked} inherited={(k) => (teamModel(k) ? `${modelLabel(k, teamModel(k))}, from the team` : modelLabel(k, null))} onChange={(k, m) => edit((d) => setGroupModel(d, groupId, k, m), `group-model:${groupId}:${k}`)} />
      <LockNote locked={locked} />
    </div>
  );
}

/** Agents the team's model does not reach (their own or their group's model wins), and a way back. */
function OwnModels({ draft, edit, locked }: Common) {
  const own = notOnTeamModel(draft);
  if (own.length === 0) return null;
  return (
    <div className="fld-hint own-models" role="status">
      <p>
        {own.length === 1 ? '1 agent uses' : `${own.length} agents use`} its own model, which the team model does not change:{' '}
        {own.map((a) => `${a.name} (${a.model})`).join(', ')}.
      </p>
      <Btn disabled={locked} onClick={() => edit(toTeamModels)}>Use the team models for every agent</Btn>
    </div>
  );
}
