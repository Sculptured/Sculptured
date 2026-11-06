import { Link } from 'react-router-dom';
import { runPath } from '../../arena/paths';
import { agoWords } from '../../arena/words';
import { compactNumber } from '../../format';
import type { OverviewTeam } from '../../generated/OverviewTeam';
import { controlPath } from '../../paths';
import { clockAt } from '../context';
import { Spark } from '../panes/Spark';
import { outcomeOf } from '../simulatedFeed';
import { useOrg } from './data';
import { byAttention, recentRuns, unitAlarms, unitRate, unitState } from './units';

/**
 * The organisation overview's panes: every deployment as a unit (state, alarms, key figures) and
 * Arena's recent runs and test suites. Each unit opens its own Control (display level 2).
 */

function Cost({ t }: { t: OverviewTeam }) {
  if (!t.started) return <span className="muted">—</span>;
  if (t.run_cost_usd == null) return <span className="muted">{t.run_tokens ? `${compactNumber(t.run_tokens)} tokens · cost unknown` : 'unknown'}</span>;
  return <>{t.run_cost_complete ? '' : '≥ '}${t.run_cost_usd.toFixed(2)} <span className="muted">· {compactNumber(t.run_tokens)} tokens</span></>;
}

function Unit({ t, now }: { t: OverviewTeam; now: number }) {
  const s = unitState(t);
  const alarms = unitAlarms(t);
  const rate = unitRate(t);
  const last = t.last_action;
  return (
    <tr className={alarms.count ? 'is-abnormal' : undefined}>
      <td><Link to={controlPath(t.id)} className="org-unit-name">{t.name}</Link></td>
      <td><span className={`ctl-glyph tone-${s.tone}`} aria-hidden="true">{s.glyph}</span> {s.word}</td>
      <td className="ctl-num">{t.running} / {t.agents}</td>
      <td>{alarms.count ? <span className="pl-abn" title={alarms.words.join('\n')}>▲ {alarms.count} · {alarms.words[0]}</span> : <span className="muted">○ none</span>}</td>
      <td className="org-rate"><Spark values={t.actions_per_min} label={`${t.name}: tool calls per minute, last 15 minutes: ${t.actions_per_min.join(', ')}`} /><span className="ctl-num">{rate.now}/min</span></td>
      <td>{last ? <><time className="ctl-t">{clockAt(Number(last.at_ms))}</time> <span className="ctl-mono">{last.tool}</span> <span className="muted">· {agoWords(Number(last.at_ms), now)}</span></> : <span className="muted">none yet</span>}</td>
      <td className="ctl-num"><Cost t={t} /></td>
    </tr>
  );
}

export function DeploymentsPane() {
  const { overview, overviewError } = useOrg();
  if (!overview) return <p className="ctl-empty">{overviewError ? `The deployments could not be read: ${overviewError}` : 'Reading the deployments…'}</p>;
  if (overview.teams.length === 0) return <p className="ctl-empty">No projects yet. Each project you make shows here as a unit with its state, alarms and figures.</p>;
  const teams = [...overview.teams].sort(byAttention);
  return (
    <div className="org-units">
      {overviewError && <p className="ctl-note">Showing the last reading: {overviewError}</p>}
      <table className="ctl-table" aria-label="Deployments">
        <thead><tr><th scope="col">Deployment</th><th scope="col">State</th><th scope="col" className="ctl-num">Running</th><th scope="col">Alarms</th><th scope="col">Tool calls</th><th scope="col">Last action</th><th scope="col" className="ctl-num">Latest run cost</th></tr></thead>
        <tbody>{teams.map((t) => <Unit key={t.id} t={t} now={Number(overview.at_ms)} />)}</tbody>
      </table>
    </div>
  );
}

export function ArenaRunsPane() {
  const { runs, runsError } = useOrg();
  if (!runs) return <p className="ctl-empty">{runsError ? `Arena is not answering (${runsError}). Its runs show here once it is.` : 'Reading Arena runs…'}</p>;
  const recent = recentRuns(runs);
  if (recent.length === 0) return <p className="ctl-empty">No Arena runs yet. Run a team in Arena from its Design board (Run in Arena), or start one in Arena control.</p>;
  const now = Date.now();
  return (
    <table className="ctl-table" aria-label="Recent Arena runs">
      <thead><tr><th scope="col">Run</th><th scope="col">Team</th><th scope="col">Outcome</th><th scope="col">Started</th></tr></thead>
      <tbody>
        {recent.map((r) => {
          const o = outcomeOf(r.status);
          return (
            <tr key={r.run} className={o.tone === 'bad' ? 'is-abnormal' : undefined}>
              <td><Link to={runPath(r.run, r.branch)}>Task {r.task}</Link>{r.external && <span className="muted"> · driven by its design</span>}</td>
              <td>{r.team}</td>
              <td><span className={`ctl-glyph tone-${o.tone}`} aria-hidden="true">{o.glyph}</span> {o.word}</td>
              <td className="muted">{agoWords(r.created, now)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export const SUITES_MISSING = 'Test suites are not available in this Arena yet. They show here, scenario by scenario, once Arena has suites.';

export function SuitesPane() {
  const { suites } = useOrg();
  if (!suites) return <p className="ctl-empty">Reading Arena's test suites…</p>;
  if ('missing' in suites) return <p className="ctl-empty">○ {SUITES_MISSING}</p>;
  if ('error' in suites) return <p className="ctl-empty">Arena's test suites could not be read: {suites.error}</p>;
  if (suites.rows.length === 0) return <p className="ctl-empty">No test suites yet.</p>;
  return (
    <table className="ctl-table" aria-label="Test suites">
      <thead><tr><th scope="col">Suite</th><th scope="col">Team</th><th scope="col">Status</th><th scope="col" className="ctl-num">Scenarios × repeats</th><th scope="col" className="ctl-num">Pass rate</th><th scope="col" className="ctl-num">Safety violations</th></tr></thead>
      <tbody>
        {suites.rows.map((s) => (
          <tr key={s.id} className={s.safety_violations ? 'is-abnormal' : undefined}>
            <td className="ctl-mono">{s.name ?? s.id}</td>
            <td>{s.team ?? 'unknown'}</td>
            <td>{s.status ?? 'unknown'}</td>
            <td className="ctl-num">{s.tasks != null && s.repeats != null ? `${s.tasks} × ${s.repeats}` : 'unknown'}</td>
            <td className="ctl-num">{s.pass_rate != null ? `${Math.round(s.pass_rate * 100)}%` : 'unknown'}</td>
            <td className={`ctl-num${s.safety_violations ? ' pl-abn' : ''}`}>{s.safety_violations ?? 'unknown'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
