import { usePolled } from '../usePolling';
import { api } from '../api';
import type { TeamBudget } from '../generated/TeamBudget';
import { nameForId, type NameMap } from '../vocab';
import { AgentLimits } from './AgentLimits';
import { RunMeters } from './RunMeters';
export function BudgetStatus({rig, names}: {rig:string; names:NameMap}) {
  const { data: budget, error } = usePolled<TeamBudget>(() => api.teamBudget(rig), 2000, [rig]);
  const failed = error !== '';
  // Quiet until a limit is reached; each agent's tokens show on its row.
  const perAgent = budget ? <AgentLimits rig={rig} agents={budget.agents} names={names} /> : null;
  const run = budget?.run ? <RunMeters rig={rig} run={budget.run} waiting={budget.waiting ?? []} names={names} /> : null;
  if (!budget?.output_limit) return failed ? <span className="muted">Budget status unavailable</span> : (run || perAgent) ? <>{run}{perAgent}</> : null;
  return <>{run}{perAgent}<details className="agent-budget">
    <summary>{budget.output_limit.toLocaleString()} output tokens per agent · descendants included{failed ? ' · status unavailable' : ''}</summary>
    <p>Stops an agent and its descendants when reported output reaches the limit. CLI usage reports may be delayed and overshoot. Input tokens are excluded. Usage resets on a new project run.</p>
    <ul>{budget.agents.map(a=><li key={a.seat}>{nameForId(a.seat,names)}: {a.output_used === null ? 'usage unknown' : `${a.output_used.toLocaleString()} / ${budget.output_limit!.toLocaleString()}`} {a.exhausted ? '· budget reached' : ''}</li>)}</ul>
  </details></>;
}
