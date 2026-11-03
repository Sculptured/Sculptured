import { useEffect, useState } from 'react';
import { Btn, Icon } from '../../shell/ui';
import { arena } from '../../tests/api';
import { DEFAULT_OPENROUTER_MODEL, KINDS, clearStoredTeam, loadTeam, saveTeam, slug, withSlugIds } from '../../tests/team';
import type { AgentKind, AgentSpec, Team } from '../../tests/types';

/**
 * The team new runs use: kept in this browser, saved as it changes; Reset goes back to Arena's
 * default. `onError` must be stable (a state setter).
 */
export function useTestTeam(onError: (e: unknown) => void) {
  const [team, setTeam] = useState<Team | null>(null);
  // Only the user's edits are kept; the default stays unstored, so it follows Arena's.
  const [edited, setEdited] = useState(false);
  useEffect(() => { loadTeam().then(setTeam).catch(onError); }, [onError]);
  useEffect(() => { if (edited && team) saveTeam(team); }, [edited, team]);
  const update = (fn: (t: Team) => Team) => { setEdited(true); setTeam((t) => (t ? fn(t) : t)); };
  const reset = async () => {
    setEdited(false);
    clearStoredTeam();
    try { setTeam(await arena.defaultTeam()); } catch (e) { onError(e); }
  };
  return { team, update, reset };
}

type Edit = { team: Team; update(fn: (t: Team) => Team): void; reset(): void; openOk: boolean };
type AgentEdit = { a: AgentSpec; i: number; count: number; openOk: boolean; patch(p: Partial<AgentSpec>): void; up(): void; remove(): void };

function KindAndModel({ a, openOk, patch }: Pick<AgentEdit, 'a' | 'openOk' | 'patch'>) {
  return (
    <div className="tst-row">
      <label className="tst-field">
        <span className="tst-label">Agent</span>
        <select value={a.kind} onChange={(e) => patch({ kind: e.target.value as AgentKind, model: null })}>
          {KINDS.map((k) => (
            <option key={k.value} value={k.value} disabled={k.value === 'open' && !openOk && a.kind !== 'open'}>
              {k.label}{k.value === 'open' && !openOk ? ' (needs OPENROUTER_API_KEY)' : ''}
            </option>
          ))}
        </select>
      </label>
      <label className="tst-field tst-grow">
        <span className="tst-label">Model</span>
        <input className="mono" value={a.model ?? ''} placeholder={a.kind === 'open' ? `${DEFAULT_OPENROUTER_MODEL} (default)` : 'default for this agent'}
          onChange={(e) => patch({ model: e.target.value.trim() === '' ? null : e.target.value })} />
      </label>
    </div>
  );
}

function AgentCard({ a, i, count, openOk, patch, up, remove }: AgentEdit) {
  return (
    <li className="tst-card tst-agent">
      <header className="tst-agent-head">
        <span className="tst-order" aria-hidden="true">{i + 1}</span>
        <input className="tst-agent-name" value={a.name} aria-label="Agent name" onChange={(e) => patch({ name: e.target.value })} />
        <span className="tst-spacer" />
        {i === count - 1 && <span className="tst-badge">may change the world</span>}
        <Btn size="sm" variant="ghost" onClick={up} disabled={i === 0}>{i === 0 ? 'Already first' : '↑ Earlier'}</Btn>
        <Btn size="sm" variant="ghost" icon="trash" onClick={remove} disabled={count === 1}>{count === 1 ? 'A team needs one agent' : 'Remove'}</Btn>
      </header>
      <KindAndModel a={a} openOk={openOk} patch={patch} />
      {a.kind === 'open' && <p className="muted tst-small">Any model id from openrouter.ai/models, such as <span className="mono">{DEFAULT_OPENROUTER_MODEL}</span>. Every model call is recorded.</p>}
      <label className="tst-field">
        <span className="tst-label">Instructions</span>
        <textarea rows={4} value={a.prompt} placeholder="Leave empty to use the default for its place in the chain" onChange={(e) => patch({ prompt: e.target.value })} />
      </label>
    </li>
  );
}

const newAgent = (as: AgentSpec[]): AgentSpec =>
  ({ id: slug('agent', new Set(as.map((a) => a.id))), name: `Agent ${as.length + 1}`, kind: 'claude_code', model: null, prompt: '' });

/** The agents of the team, in order: each hands its notes to the next, and only the last may change the world. */
export function TeamEditor({ team, update, reset, openOk }: Edit) {
  const setAgents = (fn: (a: AgentSpec[]) => AgentSpec[]) => update((t) => ({ ...t, agents: withSlugIds(fn(t.agents)) }));
  const card = (a: AgentSpec, i: number) => (
    <AgentCard key={i} a={a} i={i} count={team.agents.length} openOk={openOk}
      patch={(p) => setAgents((as) => as.map((x, j) => (j === i ? { ...x, ...p } : x)))}
      up={() => setAgents((as) => { const n = [...as]; [n[i - 1], n[i]] = [n[i], n[i - 1]]; return n; })}
      remove={() => setAgents((as) => as.filter((_, j) => j !== i))} />
  );
  return (
    <section className="arena-team" aria-label="Test team">
      <p className="muted tst-small">Agents work in order, each handing its notes to the next. Only the last one may change the world.</p>
      <ol className="tst-team">{team.agents.map(card)}</ol>
      <div className="arena-row">
        <Btn icon="plus" onClick={() => setAgents((as) => [...as, newAgent(as)])}>Add agent</Btn>
        <Btn variant="ghost" icon="undo" onClick={reset}>Reset to default</Btn>
        <span className="muted tst-small"><Icon name="check" size={13} /> Saved in this browser.</span>
      </div>
    </section>
  );
}
