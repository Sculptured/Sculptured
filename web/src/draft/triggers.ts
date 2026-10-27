import type { TeamDraft } from '../generated/TeamDraft';
import type { Trigger } from '../generated/Trigger';

/** No triggers leaves the field out, as the server leaves it out of a team file. */
const withTriggers = (d: TeamDraft, list: Trigger[]): TeamDraft => {
  if (list.length > 0) return { ...d, triggers: list };
  const { triggers: _gone, ...rest } = d;
  return rest;
};

const listOf = (d: TeamDraft) => d.triggers ?? [];

/** The most a team has, as the server allows. */
export const MAX_TRIGGERS = 8;

/** A new trigger for the team's first agent that starts the team when it isn't running. */
export function addTrigger(d: TeamDraft): TeamDraft {
  const taken = new Set(listOf(d).map((t) => t.id));
  let n = listOf(d).length + 1;
  while (taken.has(`trigger-${n}`)) n++;
  const t: Trigger = { id: `trigger-${n}`, agent: d.agents[0]?.id ?? '', message: '', starts_team: true };
  return withTriggers(d, [...listOf(d), t]);
}

export const updateTrigger = (d: TeamDraft, id: string, patch: Partial<Omit<Trigger, 'id'>>): TeamDraft =>
  withTriggers(d, listOf(d).map((t) => (t.id === id ? { ...t, ...patch } : t)));

export const removeTrigger = (d: TeamDraft, id: string): TeamDraft => withTriggers(d, listOf(d).filter((t) => t.id !== id));
