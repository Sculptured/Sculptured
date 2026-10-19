import { useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { LoginStarted } from '../generated/LoginStarted';

/** What to do once a Claude Code or Codex sign-in has started on a computer. Both work from any
 * browser: Codex shows a one-time code to enter on its page, and Claude Code's page shows a code
 * to paste back here when it cannot reach that computer itself. */
export function CliSignInSteps({ cli, host, started }: { cli: string; host?: string; started: LoginStarted }) {
  if (!started.url) return <p className="muted" data-raw-evidence>{started.output}</p>;
  const link = <a href={started.url} target="_blank" rel="noreferrer">Open the sign-in page</a>;
  if (started.code) return <CodeSteps link={link} code={started.code} />;
  return (
    <div className="cli-signin-steps">
      <p>{link} and sign in. It can be on any phone or computer.</p>
      {started.takes_code && <PasteCode cli={cli} host={host} />}
    </div>
  );
}

function CodeSteps({ link, code }: { link: React.ReactNode; code: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => void navigator.clipboard?.writeText(code).then(() => setCopied(true), () => setCopied(false));
  return (
    <ol className="cli-signin-steps">
      <li>{link} on any phone or computer, and sign in.</li>
      <li>
        Enter this code: <code className="cli-signin-code">{code}</code>{' '}
        <button type="button" className="btn btn-sm" onClick={copy}>{copied ? 'Copied' : 'Copy code'}</button>
      </li>
    </ol>
  );
}

/** Claude Code's page may end on a code; it goes to the sign-in waiting on that computer. */
function PasteCode({ cli, host }: { cli: string; host?: string }) {
  const [code, setCode] = useState('');
  const [state, setState] = useState<{ busy: boolean; sent: boolean; error: string }>({ busy: false, sent: false, error: '' });
  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    setState({ busy: true, sent: false, error: '' });
    try {
      await api.cliLoginCode(cli, code.trim(), host);
      setState({ busy: false, sent: true, error: '' });
    } catch (err) {
      setState({ busy: false, sent: false, error: errorText(err) });
    }
  };
  return (
    <form className="cli-signin-paste" onSubmit={(e) => void send(e)}>
      <label>If the page shows a code, paste it here: <input aria-label="Code from the sign-in page" value={code} onChange={(e) => setCode(e.target.value)} autoComplete="off" spellCheck={false} /></label>
      <button type="submit" className="btn btn-sm" disabled={state.busy || !code.trim()}>Send code</button>
      {state.sent && <p className="muted">Code sent. This updates by itself once the sign-in finishes.</p>}
      {state.error && <p className="tone-red" role="alert">{state.error}</p>}
    </form>
  );
}
