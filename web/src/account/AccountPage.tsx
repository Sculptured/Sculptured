import { BackToApp } from '../hosted/BackToApp';
import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { api } from '../api';
import { errorText } from '../errors';
import type { AccountUsage } from '../generated/AccountUsage';
import { useHosted } from '../hosted/HostedGate';
import { gw } from '../hosted/session';
import { UsageSummary } from './UsageSummary';
import { CliAccounts } from '../shell/CliAccounts';
import './account.css';

/** Who you are (hosted beta) with Sign out, and your own use of Sculptured. */
export function AccountPage() {
  const s = useHosted();
  const [usage, setUsage] = useState<AccountUsage | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { api.accountUsage().then(setUsage, (e) => setError(errorText(e))); }, []);
  return (
    <main className="acct-page">
      <BackToApp />
      <p><Link to="/">← Projects</Link></p>
      <h1>Your account</h1>
      {s.kind === 'signed-in' ? <SignedIn name={s.user.name} email={s.user.email} admin={s.user.role === 'admin'} /> :
        <p className="muted">Sculptured runs on this computer, so there is no account to sign in to. Your usage is below.</p>}
      <section aria-label="Agent sign-in and plan usage">
        <h2>Agent sign-in and plan usage</h2>
        <p className="muted">Whether Claude Code and Codex are signed in on this computer, and how much of each plan is used, as each CLI reports it.</p>
        <CliAccounts />
      </section>
      <section aria-label="Your usage">
        <h2>Your usage</h2>
        <p className="muted">Everything your agents used in Sculptured, from what Claude Code and Codex reported.</p>
        {error && <p className="tone-red" role="alert">{error}</p>}
        {usage ? <UsageSummary u={usage} /> : !error && <p className="muted">Loading…</p>}
      </section>
    </main>
  );
}

function SignedIn({ name, email, admin }: { name: string; email: string; admin: boolean }) {
  const [busy, setBusy] = useState(false);
  const signOut = () => { setBusy(true); void gw('POST', '/logout').finally(() => window.location.assign('/login')); };
  return (
    <section aria-label="Signed in" className="acct-who">
      <div><strong>{name}</strong><span className="muted">{email}</span>{admin && <span className="muted">Admin</span>}</div>
      <div className="acct-actions">
        <Link to="/computers">Computers</Link>
        <Link to="/what-we-record">What we record</Link>
        <button type="button" className="btn" disabled={busy} onClick={signOut}>Sign out</button>
      </div>
    </section>
  );
}
