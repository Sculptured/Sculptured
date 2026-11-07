import type { TeamDraft } from '../generated/TeamDraft';

/** What is left of a draft once these agents are gone: their connections, input, and workflow references too. */
function withoutWorkflowReferences(w: NonNullable<TeamDraft['workflow']>, removed: Set<string>) {
  const stages = new Set(w.stages.filter(stage => removed.has(stage.agent)).map(stage => stage.id));
  const keep = (id: string) => (removed.has(id) ? '' : id);
  return {
    ...w, dispatcher: keep(w.dispatcher), collector: keep(w.collector), entry: stages.has(w.entry) ? '' : w.entry,
    stages: w.stages.filter(stage => !stages.has(stage.id)).map(stage => ({
      ...stage, workers: stage.workers?.filter(id => !removed.has(id)),
      routes: Object.fromEntries(Object.entries(stage.routes).filter(([, to]) => to === null || !stages.has(to))),
    })),
  };
}

export function withoutAgents(d: TeamDraft, removed: Set<string>): TeamDraft {
  return {
    ...d, agents: d.agents.filter(a => !removed.has(a.id)), connections: d.connections.filter(c => !removed.has(c.from) && !removed.has(c.to)),
    ...(d.input ? { input: { ...d.input, to: d.input.to.filter(a => !removed.has(a)) } } : {}),
    ...(d.workflow ? { workflow: withoutWorkflowReferences(d.workflow, removed) } : {}),
  };
}
