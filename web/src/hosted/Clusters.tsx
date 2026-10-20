import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { gw } from './session';

/** A cluster the server reaches over SSH to set up and keep alive this account's runner there. Saved
 * keys and passwords never come back from the server: only the public half of a key it made. */
export type Cluster = {
  id: string; name: string; host: string; port: number; username: string;
  jump: { host: string; port: number; username: string } | null;
  auth: 'generated' | 'key' | 'password';
  public_key: string | null;
  authorized_keys: { line: string; command: string; jump_line: string | null } | null;
  saved: { key: boolean; password: boolean };
  host_key: string | null; jump_host_key: string | null;
  state: 'new' | 'connecting' | 'needs_answer' | 'running' | 'needs_code' | 'failed';
  detail: string; busy: boolean; step: string | null;
  question: { host: string; name: string; instructions: string; prompts: { text: string; echo: boolean }[] } | null;
  needs_mfa: boolean; keeper: 'tmux' | 'nohup' | null; installed: string | null;
  doctor: string | null; doctor_at: string | null; started_at: string | null; last_attempt_at: string | null; failures: number;
  created_at: string; device_id: string | null; connected: boolean;
  events: { at: string; action: string; ok: boolean; detail: string }[];
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

type Auth = Cluster['auth'];
const EMPTY = { name: '', host: '', port: '22', username: '', jump_host: '', jump_port: '22', jump_username: '', private_key: '', passphrase: '', password: '' };
type Fields = typeof EMPTY;
type Setter = (k: keyof Fields) => (e: { target: { value: string } }) => void;

/** What Add sends: only the chosen way to sign in, and the jump host only when ticked. */
function addBody(f: Fields, auth: Auth, jump: boolean): Record<string, unknown> {
  const body: Record<string, unknown> = { name: f.name || undefined, host: f.host, port: Number(f.port) || 22, username: f.username, auth };
  if (jump && f.jump_host) Object.assign(body, { jump_host: f.jump_host, jump_port: Number(f.jump_port) || 22, jump_username: f.jump_username || undefined });
  if (auth === 'key') Object.assign(body, { private_key: f.private_key, passphrase: f.passphrase || undefined });
  if (auth === 'password') body.password = f.password;
  return body;
}

/** The Add form's state; a saved secret leaves the page at once. A pasted key or password connects
 * right away; a key the server made must be added on the cluster first. */
function useAddCluster(onDone: (added: boolean) => void) {
  const [f, setF] = useState(EMPTY);
  const [auth, setAuth] = useState<Auth>('generated');
  const [jump, setJump] = useState(false);
  const [error, setError] = useState('');
  const [sending, setSending] = useState(false);
  const set: Setter = (k) => (e) => setF((x) => ({ ...x, [k]: e.target.value }));
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(''); setSending(true);
    try {
      const c = await gw<Cluster>('POST', '/clusters', addBody(f, auth, jump));
      setF((x) => ({ ...x, private_key: '', passphrase: '', password: '' }));
      if (auth !== 'generated') await gw('POST', `/clusters/${c.id}/connect`).catch(() => {});
      onDone(true);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSending(false);
    }
  };
  return { f, set, auth, setAuth, jump, setJump, error, sending, submit };
}

/** Host, port, user, an optional jump host, and how to sign in. */
function AddCluster({ onDone }: { onDone: (added: boolean) => void }) {
  const a = useAddCluster(onDone);
  return (
    <form className="hosted-form cluster-add" onSubmit={(e) => void a.submit(e)} aria-label="Add a cluster">
      <p className="tone-amber">First check your site&apos;s rules on long-running programs on login nodes. The runner is small and mostly waits, but some sites forbid AI tools or background programs there, or ask for scrontab or a service node instead.</p>
      <label>Login node<input required value={a.f.host} onChange={a.set('host')} placeholder="login.cluster.example.edu" autoComplete="off" /></label>
      <label>Port<input inputMode="numeric" value={a.f.port} onChange={a.set('port')} /></label>
      <label>User name<input required value={a.f.username} onChange={a.set('username')} autoComplete="off" /></label>
      <label>Name in Sculptured (optional)<input value={a.f.name} onChange={a.set('name')} placeholder="Like Perlmutter" /></label>
      <label className="check"><input type="checkbox" checked={a.jump} onChange={(e) => a.setJump(e.target.checked)} />Reached through a jump host</label>
      {a.jump && <JumpFields f={a.f} set={a.set} />}
      <AuthFields f={a.f} set={a.set} auth={a.auth} setAuth={a.setAuth} />
      <p className="muted">Keys and passwords are kept encrypted on the server, are never shown again, and are deleted when you remove the cluster. If the cluster asks for a code at sign-in (Duo, OTP), Sculptured asks you here; the code is used once and not kept.</p>
      {a.error && <p className="tone-red" role="alert">{a.error}</p>}
      <div className="admin-actions">
        <button type="submit" className="btn btn-primary" disabled={a.sending}>Add cluster</button>
        <button type="button" className="btn" onClick={() => onDone(false)}>Cancel</button>
      </div>
    </form>
  );
}

function JumpFields({ f, set }: { f: Fields; set: Setter }) {
  return <>
    <label>Jump host<input value={f.jump_host} onChange={set('jump_host')} placeholder="gateway.example.edu" autoComplete="off" /></label>
    <label>Jump host port<input inputMode="numeric" value={f.jump_port} onChange={set('jump_port')} /></label>
    <label>User name there (if different)<input value={f.jump_username} onChange={set('jump_username')} autoComplete="off" /></label>
  </>;
}

function AuthFields({ f, set, auth, setAuth }: { f: Fields; set: Setter; auth: Auth; setAuth: (a: Auth) => void }) {
  return <>
    <fieldset>
      <legend>How Sculptured signs in</legend>
      <label className="check"><input type="radio" name="auth" checked={auth === 'generated'} onChange={() => setAuth('generated')} />A key Sculptured makes (recommended): you add its public half on the cluster</label>
      <label className="check"><input type="radio" name="auth" checked={auth === 'key'} onChange={() => setAuth('key')} />A private key you paste</label>
      <label className="check"><input type="radio" name="auth" checked={auth === 'password'} onChange={() => setAuth('password')} />A password</label>
    </fieldset>
    {auth === 'key' && <>
      <label>Private key<textarea required rows={5} value={f.private_key} onChange={set('private_key')} spellCheck={false} autoComplete="off" /></label>
      <label>Passphrase (if it has one)<input type="password" value={f.passphrase} onChange={set('passphrase')} autoComplete="off" /></label>
    </>}
    {auth === 'password' && <label>Password<input type="password" required value={f.password} onChange={set('password')} autoComplete="off" /></label>}
  </>;
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
    c.auth === 'generated' ? 'Signs in with a key Sculptured made.' : c.auth === 'key' ? 'Private key saved (encrypted).' : 'Password saved (encrypted).',
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
      {c.detail && c.state !== 'running' && !c.busy && <p className={c.state === 'failed' ? 'tone-red' : 'muted'}>{c.detail}</p>}
      {c.question && <Question c={c} onAnswer={(answers) => void post('answer', { answers })} />}
      {c.authorized_keys && (c.state === 'new' || c.state === 'failed') && <KeySteps c={c} keys={c.authorized_keys} />}
      <p className="muted">{summary(c)}</p>
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

/** The cluster's own sign-in question (MFA): answered here, sent once, then cleared from the page. */
function Question({ c, onAnswer }: { c: Cluster; onAnswer: (a: string[]) => void }) {
  const q = c.question!;
  const [answers, setAnswers] = useState<string[]>(() => q.prompts.map(() => ''));
  const send = (e: FormEvent) => { e.preventDefault(); onAnswer(answers); setAnswers(q.prompts.map(() => '')); };
  return (
    <form className="hosted-form" onSubmit={send} aria-label={`${q.host} asks`}>
      <p><strong>{q.host} asks</strong>{q.instructions ? `: ${q.instructions}` : ''}</p>
      {q.prompts.map((p, i) => (
        <label key={i}>{p.text.trim()}
          <input type={p.echo ? 'text' : 'password'} autoComplete="one-time-code" value={answers[i] ?? ''} onChange={(e) => setAnswers((a) => a.map((x, j) => (j === i ? e.target.value : x)))} />
        </label>
      ))}
      <p className="muted">Used once to sign in, never kept.</p>
      <button type="submit" className="btn btn-primary">Send answer</button>
    </form>
  );
}
