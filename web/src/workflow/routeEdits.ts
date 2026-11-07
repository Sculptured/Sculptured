import type { TeamDraft } from '../generated/TeamDraft';

/** Move a branch without changing its outcome, stage identity, or ordinary connections. */
/** `agent` is an agent id, or a stage box's node (`$stage:<id>`) for check, person, merge and review-panel stages. */
export function reconnectWorkflowRoute(draft: TeamDraft, id: string, agent: string): { draft: TeamDraft; reason?: string } {
  const w = draft.workflow;
  const box = agent.startsWith('$stage:') ? agent.slice('$stage:'.length) : null;
  if (box !== null && !w?.stages.some(s => s.id === box)) return { draft, reason: 'That stage no longer exists.' };
  if (!w || (box === null && !draft.agents.some(a => a.id === agent))) return { draft };
  const { destination, ambiguous } = routeDestination(w, agent, id, box);
  if (destination === undefined) return { draft, reason: ambiguous
    ? 'This agent owns several stages. Choose the destination in the branch details.'
    : 'Drop onto a stage agent or the final assembly agent.' };
  if (id === 'dispatch') return { draft: { ...draft, workflow: { ...w, entry: destination! } } };
  return reconnectStage(draft, w, id, destination);
}

function routeDestination(w: NonNullable<TeamDraft['workflow']>, agent: string, id: string, box: string | null) {
  const targets = w.stages.filter(s => s.agent === agent);
  const destination = box ?? (agent === w.collector && id !== 'dispatch' ? null : targets.length === 1 ? targets[0].id : undefined);
  return { destination, ambiguous: targets.length > 1 };
}

function reconnectStage(draft: TeamDraft, w: NonNullable<TeamDraft['workflow']>, id: string, destination: string | null): { draft: TeamDraft } {
  const stage = w.stages.find(s => Object.keys(s.routes).some(outcome => `${s.id}:${outcome}` === id));
  const outcome = stage && Object.keys(stage.routes).find(outcome => `${stage.id}:${outcome}` === id);
  if (!stage || outcome === undefined) return { draft };
  return { draft: { ...draft, workflow: { ...w, stages: w.stages.map(s => s.id === stage.id ? { ...s, routes: { ...s.routes, [outcome]: destination } } : s) } } };
}
