import { Link } from 'react-router-dom';
import { compactNumber } from '../format';
import type { AccountUsage } from '../generated/AccountUsage';
import { DailyChart, dayLabel } from './DailyChart';

const usd = (n: number) => (n > 0 && n < 0.01 ? '<$0.01' : `$${n.toFixed(2)}`);
const RUNTIMES: Record<string, string> = { 'claude-code': 'Claude Code', claude: 'Claude Code', codex: 'Codex', opencode: 'OpenCode' };
/** A row's cost, only when something was reported (a missing cost is not $0.00). */
const rowCost = (n: number) => (n > 0 ? ` · ${usd(n)}` : '');
const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

function Tile({ label, value, note }: { label: string; value: string; note?: React.ReactNode }) {
  return <div role="group" aria-label={label} className="acct-tile"><span className="acct-label">{label}</span><strong>{value}</strong>{note && <span className="acct-note">{note}</span>}</div>;
}

function CostNote({ u }: { u: AccountUsage }) {
  const by = Object.fromEntries(u.cost.by_source.map((s) => [s.source, s.usd]));
  return (
    <>
      {by['claude-code'] !== undefined && <span>{usd(by['claude-code'])} reported by Claude Code</span>}
      {by['openai-list-prices'] !== undefined && <span>{usd(by['openai-list-prices'])} at OpenAI list prices (checked {u.cost.openai_prices_checked})</span>}
      {u.cost.reports_without_cost > 0 && <span>{plural(u.cost.reports_without_cost, 'report had', 'reports had')} no cost, so the real total is higher</span>}
    </>
  );
}

/** Your use across every team: what it cost, when, with which CLI and on which teams. */
export function UsageSummary({ u }: { u: AccountUsage }) {
  const t = u.totals;
  const total = t.input + t.output + t.cache_read + t.cache_write;
  if (total === 0) return <p className="muted">No agent has used any tokens yet. Start a team and its use shows up here.</p>;
  const busiest = u.by_day.reduce((a, b) => (b.tokens > a.tokens ? b : a));
  return (
    <>
      <div className="acct-tiles">
        <Tile label="Tokens used" value={compactNumber(total)} note={`${compactNumber(t.input)} input · ${compactNumber(t.output)} output · ${compactNumber(t.cache_read)} cache read · ${compactNumber(t.cache_write)} cache write`} />
        {/* No report had a cost (older runs, or a model without a list price): say so, never "$0.00". */}
        <Tile label="Cost" value={u.cost.by_source.length ? usd(u.cost.usd) : 'Unknown'} note={<CostNote u={u} />} />
        <Tile label="Turns completed" value={u.activity.turns_completed.toLocaleString()} />
        <Tile label="Agent sessions" value={u.activity.agent_sessions.toLocaleString()} />
        <Tile label="Messages you sent" value={u.activity.messages_you_sent.toLocaleString()} />
        <Tile label="Teams" value={u.activity.teams.toLocaleString()} />
      </div>
      <h3>Last 30 days</h3>
      {busiest.tokens > 0 && <p className="muted">Busiest day: {dayLabel(busiest.day)}, {compactNumber(busiest.tokens)} tokens</p>}
      <DailyChart days={u.by_day} />
      <Breakdown u={u} total={total} />
    </>
  );
}

function Breakdown({ u, total }: { u: AccountUsage; total: number }) {
  return (
    <div className="acct-columns">
      <section>
        <h3>By CLI</h3>
        <ul aria-label="By CLI" className="acct-list">
          {u.by_runtime.map((r) => <li key={r.runtime}><span>{RUNTIMES[r.runtime] ?? r.runtime}</span><span className="acct-meter" style={{ '--share': `${(100 * r.tokens) / total}%` } as React.CSSProperties} /><span>{compactNumber(r.tokens)} tokens{rowCost(r.usd)}</span></li>)}
        </ul>
      </section>
      <section>
        <h3>Teams that used the most</h3>
        <ul aria-label="Teams that used the most" className="acct-list">
          {u.top_teams.map((t) => <li key={t.team}><Link to={`/team/${encodeURIComponent(t.team)}/structure`}>{t.name}</Link><span className="acct-meter" style={{ '--share': `${(100 * t.tokens) / total}%` } as React.CSSProperties} /><span>{compactNumber(t.tokens)} tokens{rowCost(t.usd)}</span></li>)}
        </ul>
      </section>
    </div>
  );
}
