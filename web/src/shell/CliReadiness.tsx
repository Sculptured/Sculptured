import { useCallback, useEffect, useState } from 'react';
import { api, type CliOn } from '../api';
import { errorText } from '../errors';
import type { CliStatus } from '../generated/CliStatus';
import type { LoginStarted } from '../generated/LoginStarted';
import type { TeamDraft } from '../generated/TeamDraft';
import { CliInstall } from './CliInstall';
import { CliSignInSteps } from './CliSignInSteps';
import { Btn } from './ui';

export const PRODUCT: Record<string, string> = { claude: 'Claude Code', codex: 'Codex' };

/** The CLIs a team's agents run on (`claude`, `codex`); Practice agents need none. */
export function cliNeeds(draft: TeamDraft): string[] {
  return [...new Set(draft.agents.map((a) => a.kind).filter((k): k is 'claude' | 'codex' => k === 'claude' || k === 'codex'))].sort();
}

const notReady = (c: CliStatus) => !c.installed || c.signed_in === false;

/** The needed CLIs that are missing or signed out on that computer. An unknown answer (an older
 * computer, no answer, a CLI that did not say) never holds a start: the agent says so itself then. */
export async function cliGaps(need: string[], on: CliOn): Promise<CliStatus[]> {
  if (need.length === 0) return [];
  try {
    const all = await api.clis(on);
    return Array.isArray(all) ? all.filter((c) => need.includes(c.name) && notReady(c)) : [];
  } catch {
    return [];
  }
}

/** One sentence per CLI that is not ready. */
export function gapText(gaps: CliStatus[]): string {
  return gaps.map((c) => `${PRODUCT[c.name] ?? c.name} isn't ${c.installed ? 'signed in' : 'installed'} on the computer that runs this team.`).join(' ');
}

/** Each needed CLI on that computer: ready, or signed in from here. `onReady` shows once all are. */
export function CliReadiness({ need, on, onReady }: { need: string[]; on: CliOn; onReady?: () => void }) {
  const [clis, setClis] = useState<CliStatus[] | null>(null);
  const [error, setError] = useState('');
  // Keyed by value: callers pass fresh arrays and objects on every render.
  const [needKey, onKey] = [need.join(','), typeof on === 'string' ? `host:${on}` : on ? `team:${on.team}` : ''];
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const read = useCallback(() => api.clis(on).then((c) => setClis(c.filter((x) => need.includes(x.name))), (e) => setError(errorText(e))), [needKey, onKey]);
  useEffect(() => {
    void read();
    const t = setInterval(() => void read(), 5000);
    return () => clearInterval(t);
  }, [read]);
  if (!clis) return error ? <p className="tone-red" role="alert">{error}</p> : null;
  const ready = clis.length > 0 && !clis.some(notReady);
  return (
    <div className="cli-readiness">
      <ul aria-label="Claude Code and Codex on that computer">
        {clis.map((c) => <CliRow key={c.name} c={c} on={on} />)}
      </ul>
      <p className="cli-readiness-actions">
        {!ready && <Btn size="sm" onClick={() => void read()}>Check again</Btn>}
        {ready && onReady && <Btn variant="primary" onClick={onReady}>Start team</Btn>}
      </p>
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}

function CliRow({ c, on }: { c: CliStatus; on: CliOn }) {
  const [started, setStarted] = useState<LoginStarted | null>(null);
  const [error, setError] = useState('');
  const name = PRODUCT[c.name] ?? c.name;
  const signIn = async () => {
    setError('');
    try { setStarted(await api.cliLogin(c.name, on)); } catch (e) { setError(errorText(e)); }
  };
  if (!c.installed) return <li><strong>{name}</strong>: not installed<CliInstall cli={c.name} /></li>;
  if (!notReady(c)) return <li><strong>{name}</strong>: <span aria-hidden="true">✓ </span>signed in{c.account ? ` (${c.account})` : ''}</li>;
  return (
    <li>
      <strong>{name}</strong>: not signed in{' '}
      {!started && <Btn size="sm" variant="primary" onClick={() => void signIn()}>Sign in to {name}</Btn>}
      {started && <CliSignInSteps cli={c.name} on={on} started={started} />}
      {error && <p className="tone-red" role="alert">{error}</p>}
    </li>
  );
}
