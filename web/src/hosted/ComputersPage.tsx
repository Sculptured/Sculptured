import { BackToApp } from './BackToApp';
import { useCallback, useEffect, useState } from 'react';
import { usePolling } from '../usePolling';
import { api } from '../api';
import type { HostPolicyInfo } from '../generated/HostPolicyInfo';
import type { CliStatus } from '../generated/CliStatus';
import type { LoginStarted } from '../generated/LoginStarted';
import { errorText } from '../errors';
import { CliInstall } from '../shell/CliInstall';
import { CliSignInSteps } from '../shell/CliSignInSteps';
import { gw } from './session';
import { ConnectComputer } from './ConnectComputer';
import { Clusters } from './Clusters';
import './hosted.css';

/** `scope` and `label` come from gateways that know Community access keys; older ones send neither. */
export type Computer = { id: string; name: string; platform: string; created_at: string; last_seen: string | null; connected: boolean; scope?: 'computer' | 'community'; label?: string };

export function ComputersPage() {
  const [list, setList] = useState<Computer[] | null>(null);
  const [policies, setPolicies] = useState<Record<string, HostPolicyInfo | null>>({});
  const [caps, setCaps] = useState<Record<string, number | null>>({});
  const [error, setError] = useState('');
  const load = useCallback(() => {
    gw<Computer[]>('GET', '/devices').then(setList, (e) => setError(e.message));
    api.hosts().then((hs) => { setPolicies(Object.fromEntries(hs.map((h) => [h.id, h.policy]))); setCaps(Object.fromEntries(hs.map((h) => [h.id, h.max_working ?? null]))); }, () => {});
  }, []);
  usePolling(load, 10000);
  return (
    <main className="hosted-page hosted-admin">
      <BackToApp />
      <h1>Computers</h1>
      <p className="muted">Your agents run on your own computers, with your own Claude Code and Codex sign-ins. Each computer decides which folders agents may use.</p>
      {error && <p className="tone-red" role="alert">{error}</p>}
      <ul className="computer-list">
        {(list ?? []).map((c) => <ComputerRow key={c.id} c={c} policy={policies[c.id] ?? null} cap={caps[c.id] ?? null} onChange={load} onError={setError} />)}
      </ul>
      {list && list.length === 0 && <p>No computers yet.</p>}
      <section aria-label="Connect a computer">
        <h2>Connect a computer</h2>
        <p className="muted">Your agents run on your own computer, with its Claude Code and Codex sign-ins.</p>
        <ConnectComputer />
      </section>
      <Clusters />
    </main>
  );
}

const CAPS: Record<string, string> = { git: 'Git', copies: "Agents' own copies", checks: 'Checks', cli_sign_in: 'Sign-in from the browser', plan_usage: 'Plan usage', generate: 'Generating teams with AI', usage_reports: 'Where tokens went' };

/** One computer: its state, Disconnect, and when connected what it allows and its cap. */
function ComputerRow({ c, policy, cap, onChange, onError }: { c: Computer; policy: HostPolicyInfo | null; cap: number | null; onChange: () => void; onError: (m: string) => void }) {
  const name = c.label ?? c.name;
  return (
    <li>
      <strong>{name}</strong> <span className="muted">{c.platform}</span>{' '}
      {c.scope === 'community' ? <span className="muted">Uses Community only; runs no agents</span>
        : <span className={c.connected ? 'tone-green' : 'muted'}>{c.connected ? 'Connected' : c.last_seen ? `Last seen ${new Date(c.last_seen).toLocaleString()}` : 'Never connected'}</span>}
      <button className="btn" aria-label={`Disconnect ${name}`} onClick={() => void gw('POST', `/devices/${c.id}/revoke`).then(onChange, (e) => onError(e.message))}>Disconnect</button>
      {c.connected && policy && <ComputerDetails id={c.id} name={c.name} policy={policy} />}
      {c.connected && policy && <WorkingCap id={c.id} name={c.name} cap={cap} />}
    </li>
  );
}

/** Most agents working at once on this computer, across all its teams: more work waits for a slot, so
 * several teams together stay within one plan. Empty = no cap. */
function WorkingCap({ id, name, cap }: { id: string; name: string; cap: number | null }) {
  const [value, setValue] = useState(cap === null ? '' : String(cap));
  const [note, setNote] = useState('');
  useEffect(() => { setValue(cap === null ? '' : String(cap)); }, [cap]);
  const save = async () => {
    const n = value.trim() === '' ? null : Number(value);
    if (n !== null && !(Number.isInteger(n) && n >= 1 && n <= 100)) { setNote('Use a whole number from 1 to 100, or leave it empty for no cap.'); return; }
    try { await api.setHostLimit(id, n); setNote(n === null ? 'Saved: no cap.' : `Saved: at most ${n} at once.`); } catch (e) { setNote((e as Error).message); }
  };
  return (
    <div className="computer-cap">
      <label className="fld"><span>Most agents working at once</span>
        <input type="number" min={1} max={100} className="fld-input" aria-label={`Most agents working at once on ${name}`} value={value} placeholder="No cap" onChange={(e) => setValue(e.target.value)} /></label>
      <button className="btn" aria-label={`Save the cap for ${name}`} onClick={() => void save()}>Save</button>
      <p className="muted">Across all your teams on this computer. More work waits for a free slot, so several teams together stay within your plan.</p>
      {note && <p role="status">{note}</p>}
    </div>
  );
}

/** A computer's Claude Code and Codex, read again every few seconds while a sign-in is under way. */
function useComputerClis(id: string, signingIn: boolean, onError: (e: string) => void) {
  const [clis, setClis] = useState<CliStatus[] | null>(null);
  useEffect(() => {
    const read = () => api.clis(id).then(setClis, (e) => onError(errorText(e)));
    void read();
    if (!signingIn) return;
    const t = setInterval(() => void read(), 3000);
    return () => clearInterval(t);
  }, [id, signingIn, onError]);
  return clis;
}

/** What a connected computer allows, and its Claude Code / Codex sign-in, signed in from here. */
function ComputerDetails({ id, name, policy }: { id: string; name: string; policy: HostPolicyInfo }) {
  const [started, setStarted] = useState<Record<string, LoginStarted>>({});
  const [error, setError] = useState('');
  const clis = useComputerClis(id, Object.keys(started).length > 0, setError);
  const signIn = async (cli: string) => {
    setError('');
    try { const s = await api.cliLogin(cli, id); setStarted((m) => ({ ...m, [cli]: s })); } catch (e) { setError(errorText(e)); }
  };
  const label = (n: string) => (n === 'claude' ? 'Claude' : 'Codex');
  return (
    <div className="computer-details">
      <p><strong>{policy.access === 'sandboxed' ? 'Sandboxed' : 'Full access'}</strong> <span className="muted">· agents start in:</span></p>
      <ul className="computer-folders">{policy.allowed_folders.map((f) => <li key={f}><code>{f}</code></li>)}</ul>
      {policy.allowed_folders.length === 0 && <p className="tone-amber">No folders allowed yet: run <code>sculptured allow &lt;folder&gt;</code> on it.</p>}
      <p className="muted">Also allowed: {policy.capabilities.map((c) => CAPS[c] ?? c).join(', ') || 'nothing else'}.</p>
      {clis && (
        <ul className="computer-clis">
          {clis.map((c) => (
            <li key={c.name}>
              {label(c.name)}: {!c.installed ? 'not installed' : c.signed_in ? `signed in${c.account ? ` (${c.account})` : ''}` : 'not signed in'}
              {c.installed && c.signed_in !== true && policy.capabilities.includes('cli_sign_in') && (
                <button className="btn" aria-label={`Sign in to ${label(c.name)} on ${name}`} onClick={() => void signIn(c.name)}>Sign in</button>
              )}
              {started[c.name] && c.signed_in !== true && <CliSignInSteps cli={c.name} on={id} started={started[c.name]} />}
              {!c.installed && <CliInstall cli={c.name} />}
            </li>
          ))}
        </ul>
      )}
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}
