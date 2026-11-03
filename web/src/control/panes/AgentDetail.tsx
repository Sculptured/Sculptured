import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useProject } from '../../pages/ProjectContext';
import { AgentActivity } from '../../pages/AgentActivity';
import { AgentConversation } from '../../pages/AgentConversation';
import { agentPagePath } from '../../paths';
import { useAgentControls } from '../../shell/AgentControls';
import type { AgentRow } from '../../shell/model';
import { useShell } from '../../shell/ShellContext';
import { signedOut } from '../../shell/SignInNeeded';
import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { useControl } from '../context';
import { AgentControlsBar } from './AgentControlsBar';

/**
 * Agent detail: one agent's conversation or activity, with its controls (pause, resume, stop, send a
 * message). It shows the selected agent; with "Follow selection" off, the one picked in Settings.
 * Several can be open side by side.
 */

export type AgentSettings = { view?: 'conversation' | 'activity'; agent?: string; follow?: boolean };

function useAgent(s: AgentSettings): string | undefined {
  const { selection } = useSelection();
  return s.follow === false ? (s.agent ?? selection.agent) : selection.agent;
}

/** Which agent: a short list to pick from, before anything is selected. */
function Pick() {
  const { source } = useControl();
  const { select } = useSelection();
  if (source.agents.length === 0) return <p className="ctl-empty">No agents yet.</p>;
  return (
    <div className="ctl-pick">
      <p className="ctl-note">Select an agent on the board, the timeline or here:</p>
      <ul className="ctl-pick-list">
        {source.agents.map((a) => (
          <li key={a.id}><button type="button" className="ctl-pick-row" onClick={() => select({ agent: a.id })}>
            <span className={`ctl-glyph tone-${a.status.tone}`} aria-hidden="true">{a.status.glyph}</span> {a.name} <span className="ctl-lane-word">{a.status.word}</span>
          </button></li>
        ))}
      </ul>
    </div>
  );
}

function Views({ row, view, setView }: { row: AgentRow; view: 'conversation' | 'activity'; setView(v: 'conversation' | 'activity'): void }) {
  const p = useProject();
  const c = useAgentControls(row.seat!);
  const feed = p.live?.feed[row.id] ?? [];
  const [search, setSearch] = useState<HTMLElement | null>(null);
  const inline = signedOut(row, feed);
  return (
    <>
      <div className="ctl-views" role="group" aria-label="Show">
        {(['conversation', 'activity'] as const).map((v) => (
          <button key={v} type="button" className={`ctl-seg${view === v ? ' is-on' : ''}`} aria-pressed={view === v} onClick={() => setView(v)}>{v === 'conversation' ? 'Conversation' : 'Activity'}</button>
        ))}
        <span className="pane-search" ref={setSearch} />
      </div>
      <div className="ctl-agent-body">
        {view === 'conversation'
          ? <AgentConversation key={row.id} a={row} c={c} feed={feed} epoch={p.live?.epoch ?? 0} searchSlot={search} handOverInline={inline} askHandOver={0} />
          : <AgentActivity key={row.id} a={row} c={c} feed={feed} epoch={p.live?.epoch ?? 0} handOverInline={inline} askHandOver={0} />}
      </div>
    </>
  );
}

export function AgentDetailPane({ settings, setSettings }: PaneProps) {
  const s = settings as AgentSettings;
  const p = useProject();
  const shell = useShell();
  const { source } = useControl();
  const agent = useAgent(s);
  const line = agent ? source.agents.find((a) => a.id === agent) : undefined;
  if (!agent || !line) return <Pick />;
  const row = shell.byId[agent];
  return (
    <div className="ctl-agent" data-agent-detail={agent}>
      <header className="ctl-agent-head">
        <span className={`ctl-glyph tone-${line.status.tone}`} aria-hidden="true">{line.status.glyph}</span>
        <h3 className="ctl-agent-name">{line.name}</h3>
        <span className="ctl-lane-word">{line.status.word}{line.activity && line.activity !== line.status.word ? ` · ${line.activity}` : ''}</span>
        <Link className="ctl-link" to={agentPagePath(p.team, agent)}>Full page</Link>
      </header>
      <AgentControlsBar agent={agent} withMessage={!row?.seat || s.view === 'activity'} />
      {row?.seat ? <Views row={row} view={s.view ?? 'conversation'} setView={(view) => setSettings({ ...s, view })} /> : <p className="ctl-empty">Its conversation shows here once it has run.</p>}
    </div>
  );
}

export function AgentSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as AgentSettings;
  const { source } = useControl();
  return (
    <label className="ctl-field">Agent when not following
      <select value={s.agent ?? ''} onChange={(e) => setSettings({ ...s, agent: e.target.value || undefined, follow: e.target.value ? false : s.follow })}>
        <option value="">The selected one</option>
        {source.agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
      </select>
    </label>
  );
}
