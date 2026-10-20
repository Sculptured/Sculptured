import { useCallback, useEffect, useState } from 'react';
import { gw } from './session';
import { AddCluster } from './ClusterAdd';
import { Question } from './ClusterQuestion';
import { CertificateLine } from './ClusterCertificate';

/** A cluster the server reaches over SSH to set up and keep alive this account's runner there. Saved
 * keys and passwords never come back from the server: only the public half of a key it made. */
export type Cluster = {
  id: string; name: string; host: string; port: number; username: string;
  jump: { host: string; port: number; username: string } | null;
  auth: 'generated' | 'key' | 'password' | 'certificate';
  public_key: string | null;
  authorized_keys: { line: string; command: string; jump_line: string | null } | null;
  saved: { key: boolean; password: boolean };
  host_key: string | null; jump_host_key: string | null;
  state: 'new' | 'connecting' | 'needs_answer' | 'running' | 'needs_code' | 'failed';
  detail: string; busy: boolean; step: string | null;
  question: { host: string; name: string; instructions: string; prompts: { text: string; echo: boolean }[]; expires_at?: string | null; wait_after_answer?: number } | null;
  needs_mfa: boolean; keeper: 'tmux' | 'nohup' | null; installed: string | null;
  doctor: string | null; doctor_at: string | null; started_at: string | null; last_attempt_at: string | null; failures: number;
  created_at: string; device_id: string | null; connected: boolean;
  events: { at: string; action: string; ok: boolean; detail: string }[];
  /** How long a signed-in connection is kept, and until when the current one is (null: none). */
  hold_hours?: number; held_until?: string | null;
  certificate?: { principals: string[]; key_id: string | null; valid_before: string | null; warning: 'expired' | 'soon' | null } | null;
};

type Listed = { clusters: Cluster[]; available: boolean };

/** Clusters on the Computers page: add one with only the web app, then watch the server set it up. */
export function Clusters() {
  const [data, setData] = useState<Listed | null>(null);
  const [error, setError] = useState('');
  const [adding, setAdding] = useState(false);
  const [absent, setAbsent] = useState(false);
  // A server without clusters (404) shows no Clusters section.
  const load = useCallback(() => gw<Listed>('GET', '/clusters').then((d) => { setData(d); setError(''); }, (e) => (e.status === 404 ? setAbsent(true) : setError(e.message))), []);
  const busy = data?.clusters.some((c) => c.busy) ?? false;
  useEffect(() => {
    void load();
    const t = setInterval(() => void load(), busy ? 1500 : 10000);
    return () => clearInterval(t);
  }, [load, busy]);
  if (absent) return null;
  return (
    <section aria-label="Clusters" className="clusters">
      <h2>Clusters</h2>
      <p className="muted">Sculptured&apos;s server signs in to your cluster&apos;s login node over SSH, installs the runner in <code>~/.sculptured</code>, and keeps it running there. Nothing to install on your side.</p>
      {error && <p className="tone-red" role="alert">{error}</p>}
      {(data?.clusters ?? []).map((c) => <ClusterCard key={c.id} c={c} reload={load} />)}
      {data && !data.available && <p className="tone-amber">This server is not set up to keep cluster keys yet.</p>}
      {data?.available && (adding
        ? <AddCluster onDone={(added) => { setAdding(false); if (added) void load(); }} />
        : <button type="button" className="btn" onClick={() => setAdding(true)}>Add a cluster</button>)}
    </section>
  );
}

/** Plain words for where the cluster is, with color only as a second cue. */
function status(c: Cluster): { text: string; tone: string } {
  if (c.state === 'needs_answer') return { text: 'Needs your answer', tone: 'tone-amber' };
  if (c.busy) return { text: c.step ? `${c.step}…` : 'Working…', tone: 'muted' };
  switch (c.state) {
    case 'running': return c.connected ? { text: 'Connected', tone: 'tone-green' } : { text: 'Runner started, not connected', tone: 'tone-amber' };
    case 'needs_code': return { text: 'Reconnect needs your code', tone: 'tone-amber' };
    case 'failed': return { text: 'Not connected: see why below', tone: 'tone-red' };
    default: return { text: 'Not connected yet', tone: 'muted' };
  }
}

function summary(c: Cluster): string {
  return [
    { generated: 'Signs in with a key Sculptured made.', key: 'Private key saved (encrypted).', certificate: 'Certificate and its key saved (encrypted).', password: 'Password saved (encrypted).' }[c.auth],
    c.needs_mfa ? 'Asks for a code at sign-in.' : '',
    c.keeper ? `Runner kept running with ${c.keeper}.` : '',
    c.host_key ? `Host key ${c.host_key}.` : '',
  ].filter(Boolean).join(' ');
}

function ClusterCard({ c, reload }: { c: Cluster; reload: () => void }) {
  const [error, setError] = useState('');
  const s = status(c);
  const post = (path: string, body?: unknown) => gw('POST', `/clusters/${c.id}/${path}`, body).then(reload, (e) => setError(e.message));
  const where = `${c.username}@${c.host}${c.port === 22 ? '' : `:${c.port}`}`;
  return (
    <article className="cluster-card" aria-label={c.name}>
      <header className="admin-head">
        <div><strong>{c.name}</strong> <span className="muted">{where}{c.jump ? ` through ${c.jump.username}@${c.jump.host}` : ''}</span></div>
        <span className={s.tone} role="status">{s.text}</span>
      </header>
      {c.detail && c.state !== 'running' && !c.busy && c.certificate?.warning !== 'expired' && <p className={c.state === 'failed' ? 'tone-red' : 'muted'}>{c.detail}</p>}
      {c.question && <Question c={c} onAnswer={(answers) => void post('answer', { answers })} />}
      {c.authorized_keys && (c.state === 'new' || c.state === 'failed') && <KeySteps c={c} keys={c.authorized_keys} />}
      {c.certificate && <CertificateLine c={c} reload={reload} />}
      <p className="muted">{summary(c)}</p>
      {c.held_until && <SignedIn c={c} onSignOut={() => void post('sign-out')} />}
      <CardActions c={c} onConnect={() => void post('connect')} onRemove={() => void post('remove')} />
      {error && <p className="tone-red" role="alert">{error}</p>}
      <CardHistory c={c} />
    </article>
  );
}

/** For a key the server made: the exact authorized_keys line (and the jump host's), and a command. */
function KeySteps({ c, keys }: { c: Cluster; keys: NonNullable<Cluster['authorized_keys']> }) {
  return (
    <div className="cluster-key">
      <p>Add this line to ~/.ssh/authorized_keys on {c.host}, then Connect:</p>
      <pre className="hosted-cmd">{keys.line}</pre>
      <p className="muted">Or run this there:</p>
      <pre className="hosted-cmd">{keys.command}</pre>
      {keys.jump_line && c.jump && <>
        <p>And this line on the jump host {c.jump.host} (it allows only a connection to {c.host}):</p>
        <pre className="hosted-cmd">{keys.jump_line}</pre>
      </>}
    </div>
  );
}

function CardActions({ c, onConnect, onRemove }: { c: Cluster; onConnect: () => void; onRemove: () => void }) {
  const [confirming, setConfirming] = useState(false);
  const verb = c.state === 'new' ? 'Connect' : 'Reconnect';
  return <>
    <div className="admin-actions">
      {!c.busy && <button type="button" className="btn btn-primary" aria-label={`${verb} ${c.name}`} onClick={onConnect}>{verb}</button>}
      {!confirming && <button type="button" className="btn" aria-label={`Remove ${c.name}`} onClick={() => setConfirming(true)}>Remove</button>}
    </div>
    {confirming && (
      <div role="group" aria-label={`Remove ${c.name}?`} className="cluster-confirm">
        <p>Removing {c.name} deletes its saved key or password and disconnects its runner (it stops there by itself). Remove the line Sculptured added to ~/.ssh/authorized_keys yourself, and ~/.sculptured if you like.</p>
        <button type="button" className="btn" onClick={() => { setConfirming(false); onRemove(); }}>Yes, remove {c.name}</button>
        <button type="button" className="btn" onClick={() => setConfirming(false)}>Keep it</button>
      </div>
    )}
  </>;
}

/** The doctor report from the login node, and every SSH action, newest first. */
function CardHistory({ c }: { c: Cluster }) {
  return <>
    {c.doctor && (
      <details>
        <summary>sculptured doctor on {c.host}{c.doctor_at ? `, ${new Date(c.doctor_at).toLocaleString()}` : ''}</summary>
        <pre className="hosted-cmd">{c.doctor}</pre>
      </details>
    )}
    {c.events.length > 0 && (
      <details open={c.state === 'failed' || c.busy}>
        <summary>What Sculptured did ({c.events.length})</summary>
        <ol className="cluster-log" aria-label={`What Sculptured did on ${c.name}`}>
          {c.events.map((e, i) => (
            <li key={`${e.at}-${i}`}>
              <time dateTime={e.at}>{new Date(e.at).toLocaleString()}</time>{' '}
              <strong>{e.action}</strong>{!e.ok && <span className="tone-red"> (problem)</span>}: {e.detail}
            </li>
          ))}
        </ol>
      </details>
    )}
  </>;
}

/** A held connection: restarts reuse it without asking for a code until then, or until Sign out. */
function SignedIn({ c, onSignOut }: { c: Cluster; onSignOut: () => void }) {
  return (
    <p className="cluster-confirm">
      <span>Signed in until {new Date(c.held_until!).toLocaleString()}: restarts need no new code until then.</span>
      <button type="button" className="btn" aria-label={`Sign out of ${c.name}`} onClick={onSignOut}>Sign out</button>
    </p>
  );
}
