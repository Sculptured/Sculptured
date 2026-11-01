import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { Btn, Icon } from '../shell/ui';
import { arena } from './api';
import { ErrorNote, useArenaHealth } from './shared';
import { DEFAULT_OPENROUTER_MODEL, KINDS, clearStoredTeam, loadTeam, saveTeam, slug, withSlugIds } from './team';
import type { AgentKind, AgentSpec, Team } from './types';

/** `/tests/team`: the chain of agents new test runs use. Kept in this browser; changes save as you type. */
export function TeamPage() {
  const { health } = useArenaHealth();
  const [team, setTeam] = useState<Team | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loaded, setLoaded] = useState(false);
  useEffect(() => { loadTeam().then((t) => { setTeam(t); setLoaded(true); }).catch(setError); }, []);
  useEffect(() => { if (loaded && team) saveTeam(team); }, [loaded, team]);

  const openOk = !!health?.open_available;
  const setAgents = (fn: (a: AgentSpec[]) => AgentSpec[]) => setTeam((t) => (t ? { ...t, agents: withSlugIds(fn(t.agents)) } : t));
  const patch = (i: number, p: Partial<AgentSpec>) => setAgents((as) => as.map((a, j) => (j === i ? { ...a, ...p } : a)));
  const moveUp = (i: number) => setAgents((as) => { const n = [...as]; [n[i - 1], n[i]] = [n[i], n[i - 1]]; return n; });
  const remove = (i: number) => setAgents((as) => as.filter((_, j) => j !== i));
  const add = () => setAgents((as) => [...as, { id: slug('agent', new Set(as.map((a) => a.id))), name: `Agent ${as.length + 1}`, kind: 'claude_code', model: null, prompt: '' }]);
  const reset = async () => {
    clearStoredTeam();
    try { setTeam(await arena.defaultTeam()); } catch (e) { setError(e); }
  };

  return (
    <main className="page page-form tst-page">
      <nav className="tst-crumbs" aria-label="Breadcrumb"><Link to="/tests">Tests</Link><span aria-hidden="true">/</span><span>Test team</span></nav>
      <header className="tst-head">
        <div>
          <h1 className="page-title">Test team</h1>
          <p className="muted tst-lede">Agents work in order, each handing its notes to the next. Only the last one may change the world. New runs use this team; it is kept in this browser.</p>
        </div>
        <Btn variant="ghost" icon="undo" onClick={reset}>Reset to default</Btn>
      </header>
      <ErrorNote error={error} />
      {!team && !error && <p className="muted">Loading…</p>}
      {team && (
        <ol className="tst-team">
          {team.agents.map((a, i) => (
            <li key={i} className="tst-card tst-agent">
              <header className="tst-agent-head">
                <span className="tst-order" aria-hidden="true">{i + 1}</span>
                <input className="tst-agent-name" value={a.name} aria-label="Agent name" onChange={(e) => patch(i, { name: e.target.value })} />
                <span className="tst-spacer" />
                {i === team.agents.length - 1 && <span className="tst-badge">may change the world</span>}
                <Btn size="sm" variant="ghost" onClick={() => moveUp(i)} disabled={i === 0} tip={i === 0 ? 'Already first' : 'Move earlier'}>↑ Earlier</Btn>
                <Btn size="sm" variant="ghost" icon="trash" onClick={() => remove(i)} disabled={team.agents.length === 1} tip={team.agents.length === 1 ? 'A team needs at least one agent' : 'Remove this agent'}>Remove</Btn>
              </header>
              <div className="tst-row">
                <label className="tst-field">
                  <span className="tst-label">Agent</span>
                  <select value={a.kind} onChange={(e) => patch(i, { kind: e.target.value as AgentKind, model: null })}>
                    {KINDS.map((k) => (
                      <option key={k.value} value={k.value} disabled={k.value === 'open' && !openOk && a.kind !== 'open'}>
                        {k.label}{k.value === 'open' && !openOk ? ' (needs OPENROUTER_API_KEY)' : ''}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="tst-field tst-grow">
                  <span className="tst-label">Model</span>
                  <input className="mono" value={a.model ?? ''}
                    placeholder={a.kind === 'open' ? `${DEFAULT_OPENROUTER_MODEL} (default)` : 'default for this agent'}
                    onChange={(e) => patch(i, { model: e.target.value.trim() === '' ? null : e.target.value })} />
                </label>
              </div>
              {a.kind === 'open' && <p className="muted tst-small">Any model id from openrouter.ai/models, such as <span className="mono">{DEFAULT_OPENROUTER_MODEL}</span>. Every model call is recorded.</p>}
              <label className="tst-field">
                <span className="tst-label">Instructions</span>
                <textarea rows={6} value={a.prompt} placeholder="Leave empty to use the default for its place in the chain" onChange={(e) => patch(i, { prompt: e.target.value })} />
              </label>
            </li>
          ))}
        </ol>
      )}
      {team && <Btn icon="plus" onClick={add}>Add agent</Btn>}
      {team && <p className="muted tst-small"><Icon name="check" size={13} /> Saved in this browser.</p>}
    </main>
  );
}
