import { useState, type FormEvent } from 'react';
import { gw } from './session';
import { DOCS_URL } from '../paths';
import type { Cluster } from './Clusters';

type Auth = Cluster['auth'];
const EMPTY = { name: '', host: '', port: '22', username: '', jump_host: '', jump_port: '22', jump_username: '', private_key: '', passphrase: '', password: '', certificate: '', hold: '12' };
type Fields = typeof EMPTY;
type Setter = (k: keyof Fields) => (e: { target: { value: string } }) => void;

/** What Add sends: only the chosen way to sign in, and the jump host only when ticked. */
function addBody(f: Fields, auth: Auth, jump: boolean): Record<string, unknown> {
  const hold = Number(f.hold);
  const body: Record<string, unknown> = { name: f.name || undefined, host: f.host, port: Number(f.port) || 22, username: f.username, auth, hold_hours: Number.isFinite(hold) ? hold : 12 };
  if (jump && f.jump_host) Object.assign(body, { jump_host: f.jump_host, jump_port: Number(f.jump_port) || 22, jump_username: f.jump_username || undefined });
  if (auth === 'key' || auth === 'certificate') Object.assign(body, { private_key: f.private_key, passphrase: f.passphrase || undefined });
  if (auth === 'certificate') body.certificate = f.certificate;
  if (auth === 'password') body.password = f.password;
  return body;
}

/** The Add form's state; a saved secret leaves the page at once. A pasted key, certificate or password
 * connects right away; a key the server made must be added on the cluster first. */
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
      setF((x) => ({ ...x, private_key: '', passphrase: '', password: '', certificate: '' }));
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

/** Host, port, user, an optional jump host, how to sign in, and how long to stay signed in. */
export function AddCluster({ onDone }: { onDone: (added: boolean) => void }) {
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
      <label>Keep signed in for (hours)<input type="number" min={0} max={168} value={a.f.hold} onChange={a.set('hold')} /></label>
      <p className="muted">After you answer a code once, Sculptured keeps that connection open (up to 7 days; 0 keeps none) and restarts the runner over it without asking again. Many sites limit how long a session may stay open, so it may end sooner.</p>
      <p className="muted">Keys, certificates and passwords are kept encrypted on the server, are never shown again, and are deleted when you remove the cluster. If the cluster asks for a code at sign-in (Duo, OTP), Sculptured asks you here; the code is used once and not kept.</p>
      <CannotWork />
      {a.error && <p className="tone-red" role="alert">{a.error}</p>}
      <div className="admin-actions">
        <button type="submit" className="btn btn-primary" disabled={a.sending}>Add cluster</button>
        <button type="button" className="btn" onClick={() => onDone(false)}>Cancel</button>
      </div>
    </form>
  );
}

/** Sign-ins that need something on your own computer, which a web app does not have. */
function CannotWork() {
  return (
    <p className="muted">
      Not possible from a web app: hardware security keys (FIDO, <code>sk-ed25519</code> keys), Kerberos (GSSAPI) tickets and SSH agent forwarding all need your own computer. For a site that requires them, <a href={`${DOCS_URL}/connect-a-computer#set-up-a-cluster-yourself`} target="_blank" rel="noreferrer">Set up a cluster yourself</a>.
    </p>
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
  const choice = (a: Auth, title: string, hint: string) => (
    <label className={`auth-option${auth === a ? ' on' : ''}`}>
      <input type="radio" name="auth" checked={auth === a} onChange={() => setAuth(a)} />
      <span className="radio-mark" aria-hidden="true" />
      <span><strong>{title}</strong><small>{hint}</small></span>
    </label>
  );
  return <>
    <fieldset className="auth-options">
      <legend>How Sculptured signs in</legend>
      {choice('generated', 'Key Sculptured makes', 'You add one line to ~/.ssh/authorized_keys on the cluster; no password is kept.')}
      {choice('password', 'Password', 'Your site password. Codes and Duo pushes are asked when you connect. Use this when the site takes no keys, like NCSA Delta.')}
      {choice('certificate', 'Certificate from your site', 'A private key and its -cert.pub, for example from NERSC sshproxy or step-ca.')}
      {choice('key', 'Your own private key', 'Paste a key that already signs in to the cluster.')}
    </fieldset>
    {(auth === 'key' || auth === 'certificate') && <KeyFields f={f} set={set} />}
    {auth === 'certificate' && <label>Certificate (-cert.pub)<textarea required rows={3} value={f.certificate} onChange={set('certificate')} spellCheck={false} autoComplete="off" /></label>}
    {auth === 'password' && <label>Password<input type="password" required value={f.password} onChange={set('password')} autoComplete="off" /></label>}
  </>;
}

function KeyFields({ f, set }: { f: Fields; set: Setter }) {
  return <>
    <label>Private key<textarea required rows={5} value={f.private_key} onChange={set('private_key')} spellCheck={false} autoComplete="off" /></label>
    <label>Passphrase (if it has one)<input type="password" value={f.passphrase} onChange={set('passphrase')} autoComplete="off" /></label>
  </>;
}
