import {DismissibleError} from '../shell/DismissibleError';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import type { RigState } from '../useRigStream';
import { useTeamEvents } from '../useTeamEvents';
import { nameForId, type NameMap } from '../vocab';
import { agentPath, type AgentRow } from '../shell/model';
import { Btn } from '../shell/ui';
import { teamExchanges, type Exchange, type ExchangeKind } from './exchanges';
import { useConversationView } from './view';
import './conversation.css';

const LIMIT = 100;
type Props = {
  rig: string; state: RigState; connected: boolean; names: NameMap; agents: Record<string, AgentRow>;
  onOpen: (id: string) => void; onAnswer: (id: string) => void; onClose: () => void;
};

type FilterProps = { ids: string[]; nameOf: (id: string) => string; agent: string; kind: ExchangeKind | ''; changeView: (v: { agent?: string; kind?: ExchangeKind | '' }) => void; paused: boolean; pending: number; onPause: () => void };

function ExchangeFilters({ ids, nameOf, agent, kind, changeView, paused, pending, onPause }: FilterProps) {
  return <div className="tc-filters">
    <label>Agent<select value={agent} onChange={e => changeView({ agent: e.target.value })}>
      <option value="">Everyone</option>{ids.map(id => <option key={id} value={id}>{nameOf(id)}</option>)}
    </select></label>
    <label>Show<select value={kind} onChange={e => changeView({ kind: e.target.value as ExchangeKind | '' })}>
      <option value="">All communication</option><option value="updates">Updates</option>
      <option value="exchanges">Messages & handoffs</option><option value="questions">Questions</option><option value="results">Reported results</option>
    </select></label>
    <Btn aria-pressed={paused} onClick={onPause}>
      {paused ? `Resume updates${pending ? ` (${pending} new)` : ''}` : 'Pause updates'}
    </Btn>
  </div>;
}

/** The exchanges to show, with the option to freeze the list while reading. */
function useExchangeRows(rig: string, ids: string[], p: Props) {
  const { events, error } = useTeamEvents(ids, p.state.feed, p.state.epoch, 500);
  const exchanges = useMemo(() => teamExchanges(events), [events]);
  const [paused, setPaused] = useState<Exchange[] | null>(null);
  const [{ agent, kind }, changeView] = useConversationView(rig);
  const rows = (paused ?? exchanges).filter(e => (!agent || e.agent === agent || e.from.includes(agent)) && (!kind || e.kind === kind));
  const pausedKeys = useMemo(() => new Set(paused?.map(e => e.key)), [paused]);
  const pending = paused ? exchanges.filter(e => !pausedKeys.has(e.key)).length : 0;
  return { error, rows, agent, kind, changeView, paused: paused !== null, pending, togglePause: () => setPaused(paused ? null : exchanges) };
}

/** Optional board companion: actual exchanges and intermediate reports, not a run pipeline. */
export function TeamConversation(p: Props) {
  const ids = useMemo(() => p.state.snapshot?.rig.seats.map(s => s.id) ?? [], [p.state.snapshot]);
  const view = useExchangeRows(p.rig, ids, p);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus(); }, []);
  const nameOf = (id: string) => nameForId(id, p.names);
  const attention = ids.map(id => p.agents[id]).filter(a => a && a.state.key === 'needs-you');
  return <aside className="team-conversation" aria-label="Team conversation" onKeyDown={e => {
    if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); e.stopPropagation(); p.onClose(); }
  }}>
    <header className="tc-head">
      <h2 ref={heading} tabIndex={-1}>Team conversation</h2>
      <Btn onClick={p.onClose} aria-label="Close team conversation">Close</Btn>
    </header>
    <ExchangeFilters ids={ids} nameOf={nameOf} agent={view.agent} kind={view.kind} changeView={view.changeView} paused={view.paused} pending={view.pending} onPause={view.togglePause} />
    <div className="tc-scroll">
      {!p.connected && <p role="status" className="tone-amber">Reconnecting · updates may be stale.</p>}
      {p.state.gaps > 0 && <p role="status" className="tone-amber">{p.state.gaps} events lost. History may be incomplete.</p>}
      {view.error && <DismissibleError>Could not load recent history: {view.error}</DismissibleError>}
      {attention.length > 0 && <div className="tc-attention" aria-label="Agents needing you">
        <span>Needs You</span>{attention.map(a => <Btn key={a.id} onClick={() => p.onAnswer(a.id)}>Answer {a.name}</Btn>)}
      </div>}
      <p className="tc-note">Recent captured communication · newest first. Reports are not independent verification.</p>
      {view.rows.length === 0 && <p className="muted">No matching communication in the loaded history.</p>}
      <ol className="tc-list" aria-label="Team exchanges">
        {view.rows.slice(0, LIMIT).map(row => <ExchangeCard key={row.key} row={row} rig={p.rig} nameOf={nameOf} onOpen={p.onOpen} known={ids} />)}
      </ol>
      <p className="tc-note">Showing up to {LIMIT} entries from the latest 500 events per agent and live updates. Each agent’s transcript keeps the full conversation.</p>
    </div>
  </aside>;
}

function ExchangeCard({ row: r, rig, nameOf, onOpen, known }: {
  row: Exchange; rig: string; nameOf: (id: string) => string; onOpen: (id: string) => void; known: string[];
}) {
  const agentButton = (id: string) => known.includes(id)
    ? <button type="button" className="tc-person" onClick={() => onOpen(id)}>{nameOf(id)}</button>
    : <span>{nameOf(id)} (no longer on this board)</span>;
  const href = agentPath(rig, r.agent, 'conversation') + (r.item ? `?item=${encodeURIComponent(r.item)}` : '');
  return <li className={`tc-entry tc-${r.kind}`}>
    <div className="tc-route">
      {r.kind === 'exchanges' && <>{r.from.length ? r.from.map((id, i) => <span key={id}>{i > 0 && ', '}{agentButton(id)}</span>) : <span>{r.origin}</span>}<span aria-label="to">→</span></>}
      {agentButton(r.agent)}
      <time dateTime={new Date(r.event.at_ms).toISOString()} title={new Date(r.event.at_ms).toLocaleString()}>{new Date(r.event.at_ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
    </div>
    <div className="tc-meta"><span>{r.label}</span><span>{r.reported ? '◇ Agent-reported' : '◆ Captured exchange'}</span></div>
    {r.text.length <= 180 ? <p className="tc-text" data-raw-evidence>{r.text}</p> : <details className="tc-content"><summary data-raw-evidence>{r.text.slice(0, 180)}…</summary>
      <pre data-raw-evidence>{r.text}</pre>
    </details>}
    {r.status && <p className="tc-delivery" data-raw-evidence>{r.status}</p>}
    <div className="tc-actions"><Link to={href}>Open conversation</Link><Link to={agentPath(rig, r.agent, 'visual')}>Work evidence</Link></div>
    <details className="tc-raw" data-advanced><summary>Captured event</summary><pre>{JSON.stringify(r.event, null, 2)}</pre></details>
  </li>;
}
