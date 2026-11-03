import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useProject } from '../../pages/ProjectContext';
import { AgentActivity, type ActivityView } from '../../pages/AgentActivity';
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
 * message). Following the selection (the default), it shows the selected agent; with Follow off (the
 * switch in its head, or picking an agent in Settings), the agent it pins. Several can be open side by
 * side, each with its own view.
 */

export type AgentSettings = { view?: 'conversation' | 'activity'; activityView?: ActivityView; agent?: string; follow?: boolean };

/** Following (the default): the selected agent. Not following: the pinned one (a setting saved without one follows). */
function useAgent(s: AgentSettings): string | undefined {
  const { selection } = useSelection();
  return s.follow === false && s.agent ? s.agent : selection.agent;
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

type Set = (patch: Partial<AgentSettings>) => void;

function Views({ row, s, set }: { row: AgentRow; s: AgentSettings; set: Set }) {
  const p = useProject();
  const c = useAgentControls(row.seat!);
  const feed = p.live?.feed[row.id] ?? [];
  const [search, setSearch] = useState<HTMLElement | null>(null);
  const inline = signedOut(row, feed);
  const view = s.view ?? 'conversation';
  return (
    <>
      <div className="ctl-views" role="group" aria-label="Show">
        {(['conversation', 'activity'] as const).map((v) => (
          <button key={v} type="button" className={`ctl-seg${view === v ? ' is-on' : ''}`} aria-pressed={view === v} onClick={() => set({ view: v })}>{v === 'conversation' ? 'Conversation' : 'Activity'}</button>
        ))}
        <span className="pane-search" ref={setSearch} />
      </div>
      <div className="ctl-agent-body">
        {view === 'conversation'
          ? <AgentConversation key={row.id} a={row} c={c} feed={feed} epoch={p.live?.epoch ?? 0} searchSlot={search} handOverInline={inline} askHandOver={0} />
          : <AgentActivity key={row.id} a={row} c={c} feed={feed} epoch={p.live?.epoch ?? 0} handOverInline={inline} askHandOver={0}
              view={s.activityView ?? 'progress'} onView={(activityView) => set({ activityView })} />}
      </div>
    </>
  );
}

/** Follow selection, in view: turning it off keeps this agent here. */
function FollowSwitch({ agent, s, set }: { agent: string; s: AgentSettings; set: Set }) {
  const on = s.follow !== false;
  return (
    <label className="ctl-check ctl-follow">
      <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked ? { follow: true, agent: undefined } : { follow: false, agent })} />
      Follow selection
    </label>
  );
}

function Head({ agent, s, set }: { agent: string; s: AgentSettings; set: Set }) {
  const p = useProject();
  const { source } = useControl();
  const line = source.agents.find((a) => a.id === agent)!;
  return (
    <header className="ctl-agent-head">
      <span className={`ctl-glyph tone-${line.status.tone}`} aria-hidden="true">{line.status.glyph}</span>
      <h3 className="ctl-agent-name">{line.name}</h3>
      <span className="ctl-lane-word">{line.status.word}{line.activity && line.activity !== line.status.word ? ` · ${line.activity}` : ''}</span>
      <FollowSwitch agent={agent} s={s} set={set} />
      <Link className="ctl-link" to={agentPagePath(p.team, agent)}>Full page</Link>
    </header>
  );
}

export function AgentDetailPane({ settings, setSettings }: PaneProps) {
  const s = settings as AgentSettings;
  const shell = useShell();
  const { source } = useControl();
  const agent = useAgent(s);
  if (!agent || !source.agents.some((a) => a.id === agent)) return <Pick />;
  const row = shell.byId[agent];
  const set: Set = (patch) => setSettings({ ...s, ...patch });
  return (
    <div className="ctl-agent" data-agent-detail={agent}>
      <Head agent={agent} s={s} set={set} />
      <AgentControlsBar agent={agent} withMessage={!row?.seat || s.view === 'activity'} />
      {row?.seat ? <Views row={row} s={s} set={set} /> : <p className="ctl-empty">Its conversation shows here once it has run.</p>}
    </div>
  );
}

/** Picking an agent turns Follow off (this pane keeps it); "The selected one" turns Follow back on. */
export function AgentSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as AgentSettings;
  const { source } = useControl();
  return (
    <label className="ctl-field">Agent
      <select value={s.follow === false ? (s.agent ?? '') : ''} onChange={(e) => setSettings(e.target.value ? { ...s, agent: e.target.value, follow: false } : { ...s, agent: undefined, follow: true })}>
        <option value="">The selected one (follow selection)</option>
        {source.agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
      </select>
    </label>
  );
}
