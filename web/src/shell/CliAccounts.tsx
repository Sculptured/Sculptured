import { useCallback, useState } from 'react';
import { usePolling } from '../usePolling';
import { api } from '../api';
import { errorText } from '../errors';
import type { CliStatus } from '../generated/CliStatus';
import type { LoginStarted } from '../generated/LoginStarted';
import type { PlanUsageView } from '../generated/PlanUsageView';
import type { PlanWindow } from '../generated/PlanWindow';
import { CliInstall } from './CliInstall';
import { CliSignInSteps } from './CliSignInSteps';
import { Btn } from './ui';

const LABEL: Record<string, string> = { claude: 'Claude', codex: 'Codex' };
/** controld holds new work (team starts, subagents) from this percent (`plan_usage::HOLD_PERCENT`). */
const HOLD = 90;

function word(c: CliStatus): string {
  if (!c.installed) return 'not installed';
  if (c.signed_in === true) return c.account ? `signed in (${c.account})` : 'signed in';
  if (c.signed_in === false) return 'not signed in';
  return 'sign-in unknown';
}

const WINDOW: Record<string, string> = { five_hour: '5-hour', seven_day: 'weekly' };

function span(ms: number): string {
  const m = Math.max(0, Math.round(ms / 60000));
  if (m < 60) return `${m} min`;
  if (m < 1440) return `${Math.floor(m / 60)} h ${m % 60} min`;
  return `${Math.floor(m / 1440)} d ${Math.floor((m % 1440) / 60)} h`;
}

/** Windows that have not reset yet; a reset window's number no longer says anything. */
function live(u: PlanUsageView | undefined, now: number): PlanWindow[] {
  return (u?.usage.windows ?? []).filter((w) => w.resets_at === null || w.resets_at * 1000 > now);
}

const tone = (p: number | null) => (p === null ? '' : p >= HOLD ? 'tone-red' : p >= 70 ? 'tone-amber' : 'tone-green');

/** This computer's CLIs and their plan usage, polled (faster while a sign-in is under way). */
function useCliStatus(fast: boolean) {
  const [clis, setClis] = useState<CliStatus[] | null>(null);
  const [usage, setUsage] = useState<PlanUsageView[]>([]);
  const load = useCallback(() => {
    void api.clis().then((c) => setClis(Array.isArray(c) ? c : null), () => setClis(null));
    void api.planUsage().then((u) => setUsage(Array.isArray(u) ? u : []), () => setUsage([]));
  }, []);
  usePolling(load, fast ? 3000 : 60000);
  return { clis, usage };
}

/**
 * In the top bar's Account link: a small ▲ while an installed CLI is not signed in, so a signed-out
 * Claude Code or Codex is noticed before a start fails. Account and usage has the details.
 */
export function SignInAlert() {
  const { clis } = useCliStatus(false);
  const out = (clis ?? []).filter((c) => c.installed && c.signed_in === false);
  if (out.length === 0) return null;
  const said = out.map((c) => `${LABEL[c.name] ?? c.name} not signed in`).join(', ');
  return <span className="tone-amber tb-alert" data-tip={said}><span aria-hidden="true">▲</span><span className="sr-only"> {said}</span></span>;
}

const nameOf = (cli: string) => LABEL[cli] ?? cli;

function WindowLine({ w, now }: { w: PlanWindow; now: number }) {
  return (
    <li className={tone(w.used_percent)}>
      {WINDOW[w.name] ?? w.name.replace(/_/g, ' ')}: {w.used_percent === null ? 'unknown' : `${Math.round(w.used_percent)}% used`}
      {w.resets_at !== null && <span className="muted"> · resets in {span(w.resets_at * 1000 - now)}</span>}
      {w.used_percent !== null && w.used_percent >= HOLD && <> · new work held</>}
    </li>
  );
}

function PlanWindows({ cli, u, now }: { cli: string; u: PlanUsageView | undefined; now: number }) {
  const ws = live(u, now);
  return (
    <ul className="plan-windows" aria-label={`${nameOf(cli)} plan usage`}>
      {!u && <li className="muted">Plan usage unknown: no {nameOf(cli)} session has reported it yet.</li>}
      {ws.map((w) => <WindowLine key={w.name} w={w} now={now} />)}
      {u && ws.length === 0 && <li className="muted">Every reported window has reset since; waiting for a new report.</li>}
      {u?.usage.credits && <li>Credits: {u.usage.credits} (use past the limit spends them)</li>}
      {u && (
        <li className="muted">
          Reported by the CLI {span(now - u.observed_ms)} ago
          {u.source === 'codex-session-files' ? ' (Codex session files; counts use outside Sculptured)' : ' in an agent session'}
        </li>
      )}
    </ul>
  );
}

function CliRow({ c, started, usage, now, signIn }: { c: CliStatus; started: LoginStarted | undefined; usage: PlanUsageView[]; now: number; signIn: (name: string) => void }) {
  const unsigned = c.signed_in !== true;
  return (
    <li>
      <strong>{nameOf(c.name)}</strong>: {word(c)}
      {c.version && <> · <span className="muted cli-version" title={c.version} data-raw-evidence>{c.version}</span></>}
      {c.installed && unsigned && <>{' '}<Btn size="sm" onClick={() => signIn(c.name)}>Sign in</Btn></>}
      {started && unsigned && <CliSignInSteps cli={c.name} started={started} />}
      {!c.installed && <CliInstall cli={c.name} />}
      <PlanWindows cli={c.name} u={usage.find((x) => x.usage.provider === c.name)} now={now} />
    </li>
  );
}

/** Whether this computer's Claude Code and Codex CLIs are signed in and how much of each plan is used
 *  (as the CLIs report it), with sign-in from the browser. The CLI runs its own sign-in (it opens the
 *  system browser); Sculptured stores no credentials. Shown on the Account and usage page. */
export function CliAccounts() {
  const [started, setStarted] = useState<Record<string, LoginStarted>>({});
  const [error, setError] = useState<string | null>(null);
  const { clis, usage } = useCliStatus(Object.keys(started).length > 0);
  const signIn = async (name: string) => {
    setError(null);
    try {
      const s = await api.cliLogin(name);
      setStarted((m) => ({ ...m, [name]: s }));
    } catch (e) {
      setError(errorText(e));
    }
  };
  if (!clis) return null;
  const now = Date.now();
  return (
    <div className="cli-accounts" data-ready={clis.every((c) => c.signed_in === true)}>
      <ul aria-label="Agent accounts">
        {clis.map((c) => <CliRow key={c.name} c={c} started={started[c.name]} usage={usage} now={now} signIn={signIn} />)}
      </ul>
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}
