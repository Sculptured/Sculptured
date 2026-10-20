import { useEffect, useState } from 'react';
import { api } from '../api';
import type { TeamSettings } from '../generated/TeamSettings';
import { gw } from './session';
import type { Computer } from './ComputersPage';

/** Hosted: which of your computers runs this team. */
export function RunOn({ team }: { team: string }) {
  const [computers, setComputers] = useState<Computer[]>([]);
  const [settings, setSettings] = useState<TeamSettings | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { gw<Computer[]>('GET', '/devices').then(setComputers, () => {}); api.teamSettings(team).then(setSettings, () => {}); }, [team]);
  if (!settings) return null;
  const save = async (host: string) => { setError(''); try { setSettings(await api.saveTeamSettings(team, { ...settings, host: host || null })); } catch (e) { setError((e as Error).message); } };
  return (
    <div className="fld">
      <label className="fld-label" htmlFor={`run-on-${team}`}>Run on</label>
      <select id={`run-on-${team}`} className="fld-input" value={settings.host ?? ''} onChange={(e) => void save(e.target.value)}>
        <option value="">Automatic (your only connected computer)</option>
        {computers.map((c) => <option key={c.id} value={c.id}>{c.name}{c.connected ? '' : ' (not connected)'}</option>)}
      </select>
      {computers.length === 0 && <p className="fld-hint">No computers connected yet. <a href="/computers">Connect one</a>.</p>}
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}

/** Hosted: run one agent on another of your computers than the team's (a cluster's login node, say). */
export function AgentRunOn({ team, agent, name }: { team: string; agent: string; name: string }) {
  const [computers, setComputers] = useState<Computer[]>([]);
  const [settings, setSettings] = useState<TeamSettings | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { gw<Computer[]>('GET', '/devices').then(setComputers, () => {}); api.teamSettings(team).then(setSettings, () => {}); }, [team]);
  if (!settings) return null;
  const save = async (host: string) => {
    setError('');
    const { [agent]: _old, ...others } = settings.agent_hosts ?? {};
    try {
      setSettings(await api.saveTeamSettings(team, { ...settings, agent_hosts: host ? { ...others, [agent]: host } : others }));
      window.dispatchEvent(new CustomEvent(AGENT_HOSTS_CHANGED, { detail: team }));
    } catch (e) { setError((e as Error).message); }
  };
  return (
    <div className="fld">
      <label className="fld-label" htmlFor={`run-on-${team}-${agent}`}>{name} runs on</label>
      <select id={`run-on-${team}-${agent}`} className="fld-input" value={settings.agent_hosts?.[agent] ?? ''} onChange={(e) => void save(e.target.value)}>
        <option value="">The team's computer</option>
        {computers.map((c) => <option key={c.id} value={c.id}>{c.name}{c.connected ? '' : ' (not connected)'}</option>)}
      </select>
      <p className="fld-hint">Messages and handoffs reach it there. Files stay on the computer where they were made; results arrive as text and file paths.</p>
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}

const AGENT_HOSTS_CHANGED = 'sculptured:agent-hosts';

/** Hosted: each agent that runs on its own computer, by that computer's name (for the board). */
export function useAgentComputers(team: string, enabled: boolean): Record<string, string> {
  const [on, setOn] = useState<Record<string, string>>({});
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    const load = () => Promise.all([gw<Computer[]>('GET', '/devices'), api.teamSettings(team)]).then(([computers, s]) => {
      const name = (id: string) => computers.find((c) => c.id === id)?.name ?? id;
      if (live) setOn(Object.fromEntries(Object.entries(s.agent_hosts ?? {}).map(([agent, host]) => [agent, name(host)])));
    }, () => {});
    void load();
    const changed = (e: Event) => { if ((e as CustomEvent<string>).detail === team) void load(); };
    window.addEventListener(AGENT_HOSTS_CHANGED, changed);
    return () => { live = false; window.removeEventListener(AGENT_HOSTS_CHANGED, changed); };
  }, [team, enabled]);
  return on;
}
