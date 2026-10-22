import type { AgentKind } from '../generated/AgentKind';
import type { DraftAgent } from '../generated/DraftAgent';
import type { ModelDefaults } from '../generated/ModelDefaults';
import type { TeamDraft } from '../generated/TeamDraft';

/** The kinds that call a model; Practice agents never do. */
export type ModelKind = 'claude' | 'codex';

export const isModelKind = (k: AgentKind): k is ModelKind => k !== 'practice';

/** Where an agent's model comes from, for the "from team / from group" note. */
export type ModelSource = 'agent' | 'group' | 'team' | 'cli';

const clean = (m: string | null | undefined) => (m && m.trim() ? m.trim() : null);

const pick = (m: ModelDefaults | undefined, k: ModelKind) => clean(m?.[k]);

/** The model an agent runs with: its own, else its group's, else the team's, else the CLI's default. Mirrors `TeamDraft::effective_model`. */
export function effectiveModel(d: TeamDraft, a: DraftAgent): { model: string | null; source: ModelSource } {
  const own = clean(a.advanced.model);
  if (own) return { model: own, source: 'agent' };
  if (!isModelKind(a.kind)) return { model: null, source: 'cli' };
  const group = pick(d.groups.find((g) => g.id === a.group_id)?.models, a.kind);
  if (group) return { model: group, source: 'group' };
  const team = pick(d.models, a.kind);
  return team ? { model: team, source: 'team' } : { model: null, source: 'cli' };
}

const withoutConnection = (a: DraftAgent): DraftAgent => {
  const { connection: _gone, ...rest } = a;
  return rest;
};

/** Switches one agent to another kind. Its own model and model connection belonged to the old CLI, so they are cleared. */
export const setKind = (d: TeamDraft, id: string, kind: AgentKind): TeamDraft => ({
  ...d,
  agents: d.agents.map((a) => (a.id === id ? withoutConnection({ ...a, kind, advanced: { ...a.advanced, model: null } }) : a)),
});

/** What an agent runs on: a kind and, for Claude, one of your model connections (null: the subscription). */
export function setRunsOn(d: TeamDraft, id: string, kind: AgentKind, connection: string | null): TeamDraft {
  const a = d.agents.find((x) => x.id === id);
  if (!a) return d;
  const base = a.kind === kind ? d : setKind(d, id, kind);
  return { ...base, agents: base.agents.map((x) => (x.id !== id ? x : kind === 'claude' && connection ? { ...x, connection } : withoutConnection(x))) };
}

/** Switches every agent in `ids` (default: all) to `kind`. */
export const setKindAll = (d: TeamDraft, kind: AgentKind, ids?: string[]): TeamDraft =>
  d.agents.filter((a) => !ids || ids.includes(a.id)).reduce((acc, a) => setKind(acc, a.id, kind), d);

const withModel = (m: ModelDefaults | undefined, k: ModelKind, model: string | null): ModelDefaults => ({
  claude: m?.claude ?? null,
  codex: m?.codex ?? null,
  [k]: clean(model),
});

export const setTeamModel = (d: TeamDraft, k: ModelKind, model: string | null): TeamDraft => ({
  ...d,
  models: withModel(d.models, k, model),
});

export const setGroupModel = (d: TeamDraft, groupId: string, k: ModelKind, model: string | null): TeamDraft => ({
  ...d,
  groups: d.groups.map((g) => (g.id === groupId ? { ...g, models: withModel(g.models, k, model) } : g)),
});

/** The default for agents in a group, before any agent override: the group's, else the team's. */
export const groupDefault = (d: TeamDraft, groupId: string | null, k: ModelKind): string | null =>
  pick(d.groups.find((g) => g.id === groupId)?.models, k) ?? pick(d.models, k);

/** Agents whose model is set on the agent or its group, so changing the team's model does not change theirs. */
export function notOnTeamModel(d: TeamDraft): { id: string; name: string; model: string }[] {
  return d.agents.flatMap((a) => {
    const { model, source } = effectiveModel(d, a);
    return model && (source === 'agent' || source === 'group') ? [{ id: a.id, name: a.name.trim() || 'Unnamed agent', model }] : [];
  });
}

/** Every agent and group back on the team's models: their own model settings are cleared. */
export const toTeamModels = (d: TeamDraft): TeamDraft => ({
  ...d,
  groups: d.groups.map((g) => ({ ...g, models: { claude: null, codex: null } })),
  agents: d.agents.map((a) => ({ ...a, advanced: { ...a.advanced, model: null } })),
});
