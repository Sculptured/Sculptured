import type { AgentAdvanced } from '../generated/AgentAdvanced';
import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EditResult } from '../useTeamDraft';
import { addGroup, moveAgent, moveToGroup, removeElement, setAdvanced, setFolder, takeOut } from './edits';
import { setKindAll } from './models';

/**
 * Edits to several agents at once. Each returns one new draft, so the board records it as one
 * undo step. Agents that are not on the team are skipped.
 */

const present = (d: TeamDraft, ids: string[]) => ids.filter((id) => d.agents.some((a) => a.id === id));

/**
 * Moves the agents together by (dx, dy), each the way a drag of that one card would (into the
 * group it lands on, or out). One refused move (a running team's agent cannot change group)
 * refuses them all, with its reason.
 */
export function moveAgents(d: TeamDraft, ids: string[], dx: number, dy: number, keepGroup = false): EditResult {
  let cur = d;
  for (const id of present(d, ids)) {
    const a = cur.agents.find((x) => x.id === id)!;
    const r = moveAgent(cur, id, { x: a.x + dx, y: a.y + dy }, keepGroup);
    if (r.reason) return { draft: d, reason: r.reason };
    cur = r.draft;
  }
  return cur;
}

/** Deletes the agents and their connections. */
export const removeAgents = (d: TeamDraft, ids: string[]): TeamDraft =>
  present(d, ids).reduce((acc, id) => removeElement(acc, { kind: 'agent', id }).draft, d);

/** Claude or Codex for every agent in `ids`; their own models belonged to the old kind, so they are cleared. */
export const setKindFor = (d: TeamDraft, ids: string[], kind: AgentKind): TeamDraft => setKindAll(d, kind, present(d, ids));

/** The agents' own model (null = back to their group's or team's). */
export const setModelFor = (d: TeamDraft, ids: string[], model: string | null): TeamDraft =>
  present(d, ids).reduce((acc, id) => setAdvanced(acc, id, { model }), d);

/** Each agent's own folder (null = the project folder). */
export const setFolderFor = (d: TeamDraft, ids: string[], folder: string | null): TeamDraft =>
  present(d, ids).reduce((acc, id) => setFolder(acc, id, folder), d);

/** Hard limits for one run; a soft value is left to its 85% default. undefined = unchanged, null = no limit. */
export type BulkLimits = { tokens?: number | null; turns?: number | null; minutes?: number | null };

export function setLimitsFor(d: TeamDraft, ids: string[], l: BulkLimits): TeamDraft {
  return present(d, ids).reduce((acc, id) => {
    const a = acc.agents.find((x) => x.id === id)!.advanced;
    const patch: Partial<AgentAdvanced> = {};
    if (l.tokens !== undefined)
      patch.token_limit = l.tokens === null ? undefined : { counts: a.token_limit?.counts ?? 'all', include_subagents: a.token_limit?.include_subagents ?? true, tokens: l.tokens };
    if (l.turns !== undefined) patch.turn_limit = l.turns === null ? undefined : { hard: l.turns };
    if (l.minutes !== undefined) patch.minute_limit = l.minutes === null ? undefined : { hard: l.minutes };
    return setAdvanced(acc, id, patch);
  }, d);
}

/** Puts the agents in a group (each in its first free slot), or takes them out of every group (null). */
export const moveAgentsToGroup = (d: TeamDraft, ids: string[], groupId: string | null): TeamDraft =>
  present(d, ids).reduce((acc, id) => (groupId === null ? takeOut(acc, id) : moveToGroup(acc, id, groupId)), d);

/** A new group holding the agents. */
export function groupAgents(d: TeamDraft, ids: string[]): { draft: TeamDraft; id: string } {
  const r = addGroup(d);
  return { draft: moveAgentsToGroup(r.draft, ids, r.id), id: r.id };
}
