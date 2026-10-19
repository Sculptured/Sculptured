import { api } from '../api';
import type { AgentBudget } from '../generated/AgentBudget';
import { nameForId, type NameMap } from '../vocab';
import { RaiseLimit } from './RaiseLimit';

const counts = (a: AgentBudget) => (a.token_limit!.counts === 'all' ? 'tokens' : a.token_limit!.counts === 'new' ? 'tokens excluding cache reads' : 'output tokens');

/** Agents' own limits (tokens, turns, minutes), quiet until one is reached, each raisable for this run. */
export function AgentLimits({ rig, agents, names }: { rig: string; agents: AgentBudget[]; names: NameMap }) {
  const limited = agents.filter((a) => a.member && (a.token_limit || a.turn_limit || a.minute_limit));
  if (!limited.some((a) => a.exhausted)) return null;
  return (
    <details className="agent-budget">
      <summary>Agent limits · {limited.filter((a) => a.exhausted).length} reached</summary>
      <ul>{limited.map((a) => <AgentRow key={a.seat} rig={rig} a={a} name={nameForId(a.seat, names)} />)}</ul>
    </details>
  );
}

function AgentRow({ rig, a, name }: { rig: string; a: AgentBudget; name: string }) {
  const raise = (limit: 'tokens' | 'turns' | 'minutes') => (hard: number) => api.raiseAgentLimit(rig, a.member, { limit, hard });
  return (
    <li>
      {a.token_limit && (
        <span>
          {name}: {a.token_used === null ? 'usage unknown' : a.token_used.toLocaleString()} / {a.token_limit.tokens.toLocaleString()} {counts(a)}{a.token_limit.include_subagents ? ', with agents it spawned' : ''}{a.exhausted ? ' · limit reached' : ''}{' '}
          <RaiseLimit label={`${name}’s token limit`} unit="tokens" hard={a.token_limit.tokens} continues={a.exhausted} send={raise('tokens')} />
        </span>
      )}
      {a.turn_limit && <span> {name}: at most {a.turn_limit.hard} turns <RaiseLimit label={`${name}’s turn limit`} unit="turns" hard={a.turn_limit.hard} continues={a.exhausted} send={raise('turns')} /></span>}
      {a.minute_limit && <span> {name}: at most {a.minute_limit.hard} min working <RaiseLimit label={`${name}’s minute limit`} unit="minutes" hard={a.minute_limit.hard} continues={a.exhausted} send={raise('minutes')} /></span>}
    </li>
  );
}
