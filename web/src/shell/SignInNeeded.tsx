import { useState } from 'react';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import type { AgentRow } from './model';

/** What Claude Code and Codex say when they are not signed in on the computer that runs them. */
const SIGNED_OUT = /not logged in|please run \/login|codex login|not signed in/i;

/** The agent's CLI is not signed in on its computer (seen in its latest events): say so, and sign it in
 * from here (`api.cliLogin` runs the CLI's sign-in on that computer and returns the link it prints). */
/** The agent's CLI said it is not signed in, in its latest events. */
export function signedOut(a: AgentRow, feed: EventRecord[]): boolean {
  const seat = a.seat;
  if (!seat || (seat.runtime !== 'claude-code' && seat.runtime !== 'codex')) return false;
  return feed.slice(-30).some((e) => SIGNED_OUT.test(JSON.stringify(e.body)));
}

export function SignInNeeded({ a, feed }: { a: AgentRow; feed: EventRecord[] }) {
  const [link, setLink] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const seat = a.seat;
  if (!seat || !signedOut(a, feed)) return null;
  const cli = seat.runtime === 'codex' ? 'codex' : 'claude';
  const product = cli === 'codex' ? 'Codex' : 'Claude Code';
  const start = async () => {
    setBusy(true); setError('');
    try {
      const s = await api.cliLogin(cli, seat.host ?? undefined);
      if (s.url) { setLink(s.url); window.open(s.url, '_blank', 'noopener'); } else setError(s.output || `${product} did not print a sign-in link.`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="signin-needed" role="alert">
      <p><strong>{product} isn't signed in on the computer that runs {a.name}.</strong> Sign it in once; every agent on that computer can then use it.</p>
      <p className="signin-actions">
        <button type="button" className="btn btn-primary" onClick={() => void start()} disabled={busy}>Sign in to {product}</button>
        {link && <a href={link} target="_blank" rel="noreferrer">Open the sign-in page</a>}
      </p>
      {link && <p className="muted">After signing in, press <strong>Hand over again</strong> (or Restart) so {a.name} tries again.</p>}
      {error && <p className="tone-red">{error}</p>}
    </div>
  );
}
