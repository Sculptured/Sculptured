import { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { api } from '../api';
import { useHosted } from '../hosted/HostedGate';
import { useShell } from '../shell/ShellContext';
import { Btn } from '../shell/ui';

type Props = {
  /** The hosted app (a computer must be connected); else Sculptured runs on this computer. */
  hosted: boolean;
  /** A computer is connected (null: not known yet). */
  computer: boolean | null;
  /** Claude Code or Codex is signed in on one (null: not known yet). */
  cli: boolean | null;
  /** A team has been started. */
  started: boolean;
  onHide: () => void;
};

type Step = { title: string; done: boolean; to: string; action: string };

/** The few steps from a new account to a running team, each ticked off as it happens. Gone once all are done. */
export function GettingStarted({ hosted, computer, cli, started, onHide }: Props) {
  const steps: Step[] = [
    ...(hosted ? [{ title: 'Connect a computer', done: computer === true, to: '/computers', action: 'Connect a computer' }] : []),
    { title: 'Sign in to Claude Code or Codex on it', done: cli === true, to: hosted ? '/computers' : '/account', action: 'Sign in' },
    { title: 'Start an example team', done: started, to: '/new', action: 'Pick an example' },
  ];
  if (steps.every((s) => s.done)) return null;
  return (
    <section className="getting-started panel" aria-labelledby="getting-started-title">
      <div className="getting-started-head">
        <h2 id="getting-started-title">Get your first team running</h2>
        <Btn size="sm" variant="ghost" onClick={onHide}>Hide</Btn>
      </div>
      <ol>
        {steps.map((s) => (
          <li key={s.title} className={s.done ? 'done' : ''}>
            <span className="gs-state"><span aria-hidden="true">{s.done ? '✓' : '○'} </span>{s.done ? 'Done' : 'To do'}</span>
            <span className="gs-title">{s.title}</span>
            {!s.done && <Link to={s.to} className="btn btn-sm">{s.action}</Link>}
          </li>
        ))}
      </ol>
    </section>
  );
}

const HIDDEN = 'sculptured.gettingStarted.hidden';
const readHidden = () => { try { return localStorage.getItem(HIDDEN) === '1'; } catch { return false; } };

/** The checklist with its facts read live: computers, their CLIs, and whether any team has started. */
export function GettingStartedNow() {
  const hosted = useHosted().kind === 'signed-in';
  const shell = useShell();
  const [hidden, setHidden] = useState(readHidden);
  const facts = useSetupFacts(hosted, !hidden);
  const started = (shell.summaries ?? []).some((s) => s.started !== null) || shell.rows.some((r) => r.agents.some((a) => a.seat));
  const hide = () => { setHidden(true); try { localStorage.setItem(HIDDEN, '1'); } catch { /* kept for this visit only */ } };
  if (hidden) return null;
  return <GettingStarted hosted={hosted} computer={facts.computer} cli={facts.cli} started={started} onHide={hide} />;
}

/** Whether a computer is connected and has Claude Code or Codex signed in, read again every 10 s. */
function useSetupFacts(hosted: boolean, active: boolean) {
  const [facts, setFacts] = useState<{ computer: boolean | null; cli: boolean | null }>({ computer: hosted ? null : true, cli: null });
  const read = useCallback(async () => {
    // Any failure reads as "not known yet": the checklist never claims a step it cannot see.
    const hosts = hosted ? await Promise.resolve().then(() => api.hosts()).catch(() => null) : [{ id: undefined }];
    if (!Array.isArray(hosts)) return;
    const ids = hosts.map((h) => h.id);
    const lists = await Promise.all(ids.map((id) => Promise.resolve().then(() => api.clis(id)).catch(() => [])));
    setFacts({ computer: ids.length > 0, cli: lists.some((l) => Array.isArray(l) && l.some((c) => c.signed_in === true)) });
  }, [hosted]);
  useEffect(() => {
    if (!active) return;
    void read();
    const t = setInterval(() => void read(), 10_000);
    return () => clearInterval(t);
  }, [read, active]);
  return facts;
}
