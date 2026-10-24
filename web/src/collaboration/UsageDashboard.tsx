import { useEffect, useMemo, useRef, useState, type PointerEvent } from 'react';
import { usePolled } from '../usePolling';
import { Link } from 'react-router-dom';
import { api } from '../api';
import type { AgentUsage } from '../generated/AgentUsage';
import type { TeamUsage } from '../generated/TeamUsage';
import type { UsagePoint } from '../generated/UsagePoint';
import { agentPath } from '../shell/model';
import { Btn } from '../shell/ui';
import { nameForId, type NameMap } from '../vocab';
import './usage.css';

/** Categorical slots in fixed order (validated against the app surface). */
const SERIES = ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9'];
const W = 720, H = 240, PAD = { l: 56, r: 12, t: 12, b: 28 };

const total = (p: UsagePoint | undefined) => (p ? p.input + p.output : 0);
export const tokens = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

type Series = { key: string; label: string; color: string; points: UsagePoint[] };

/** Running total (input + output) for one agent at time `t`. */
function at(points: UsagePoint[], t: number): number {
  let v = 0;
  for (const p of points) { if (p.at_ms > t) break; v = total(p); }
  return v;
}

/** The busiest agents get a line each (at most one per categorical slot; the table lists every agent).
 *  Lines are in a stable order by seat id, so a colour stays on its agent while the set is unchanged. */
export function seriesOf(agents: AgentUsage[], name: (id: string) => string): Series[] {
  const used = agents.filter((a) => a.points.length).sort((a, b) => total(b.points.at(-1)) - total(a.points.at(-1)));
  return used.slice(0, SERIES.length).sort((a, b) => a.seat.localeCompare(b.seat))
    .map((a, i) => ({ key: a.seat, label: name(a.seat), color: SERIES[i], points: a.points }));
}

/** Token use over time: each agent's running total as its CLI reported it, and the latest context fill. */
export function UsageDashboard({ rig, names, onClose }: { rig: string; names: NameMap; onClose: () => void }) {
  const { data: usage, error } = usePolled<TeamUsage>(() => api.teamUsage(rig), 10000, [rig]);
  const [hover, setHover] = useState<number | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus(); }, []);
  const name = (id: string) => nameForId(id, names);
  const agents = usage?.agents ?? [];
  const series = useMemo(() => seriesOf(agents, (id) => nameForId(id, names)), [agents, names]);
  const all = series.flatMap((s) => s.points);
  const t0 = Math.min(...all.map((p) => p.at_ms)), t1 = Math.max(t0 + 60000, ...all.map((p) => p.at_ms));
  const top = Math.max(1, ...series.map((s) => total(s.points.at(-1))));
  const x = (t: number) => PAD.l + ((t - t0) / (t1 - t0)) * (W - PAD.l - PAD.r);
  const y = (v: number) => H - PAD.b - (v / top) * (H - PAD.t - PAD.b);
  const path = (pts: UsagePoint[]) => pts.length ? `M${x(pts[0].at_ms)},${y(0)} ` + pts.map((p) => `L${x(p.at_ms)},${y(total(p))}`).join(' ') : '';
  const sum = agents.reduce((n, a) => { const p = a.points.at(-1); return p ? { input: n.input + p.input, output: n.output + p.output, cache: n.cache + p.cache_read } : n; }, { input: 0, output: 0, cache: 0 });
  const move = (e: PointerEvent<SVGSVGElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const px = ((e.clientX - r.left) / r.width) * W;
    setHover(px < PAD.l || px > W - PAD.r ? null : t0 + ((px - PAD.l) / (W - PAD.l - PAD.r)) * (t1 - t0));
  };
  const rows = [...agents].sort((a, b) => total(b.points.at(-1)) - total(a.points.at(-1)));
  return (
    <section className="communication-dashboard usage-dashboard" aria-label="Token use" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
      <header><h2 ref={heading} tabIndex={-1}>Token use</h2><span className="muted">refreshed every 10 s</span><Btn onClick={onClose}>Back to board</Btn></header>
      {error && <p role="alert" className="tone-red">Could not load token use: {error}</p>}
      <dl className="traffic-totals">
        <div><dt>Input + output</dt><dd>{tokens(sum.input + sum.output)}</dd></div>
        <div><dt>Output</dt><dd>{tokens(sum.output)}</dd></div>
        <div><dt>Cached input read</dt><dd>{tokens(sum.cache)}</dd></div>
        <div><dt>Agents reporting</dt><dd>{agents.filter((a) => a.points.length).length}</dd></div>
      </dl>
      {usage && !series.length && <p>No agent has reported token use yet.</p>}
      {series.length > 0 && (
        <figure className="usage-chart">
          <figcaption>Running total per agent (input + output tokens){agents.filter((a) => a.points.length).length > series.length ? ` · the ${series.length} busiest agents; the table lists all` : ''}</figcaption>
          <ul className="usage-legend">{series.map((s) => <li key={s.key}><span style={{ background: s.color }} aria-hidden="true" />{s.label}</li>)}</ul>
          <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Running token totals for ${series.length} lines from ${clock(t0)} to ${clock(t1)}; the table below lists every agent`}
            onPointerMove={move} onPointerLeave={() => setHover(null)}>
            {[0, 0.5, 1].map((f) => <g key={f}><line className="usage-grid" x1={PAD.l} x2={W - PAD.r} y1={y(top * f)} y2={y(top * f)} /><text className="usage-axis" x={PAD.l - 6} y={y(top * f) + 4} textAnchor="end">{tokens(Math.round(top * f))}</text></g>)}
            <text className="usage-axis" x={PAD.l} y={H - 8}>{clock(t0)}</text>
            <text className="usage-axis" x={W - PAD.r} y={H - 8} textAnchor="end">{clock(t1)}</text>
            {series.map((s) => <path key={s.key} d={path(s.points)} stroke={s.color} className="usage-line" />)}
            {series.map((s) => { const p = s.points.at(-1)!; return <circle key={s.key} cx={x(p.at_ms)} cy={y(total(p))} r={4} fill={s.color} className="usage-end" />; })}
            {hover !== null && <line className="usage-cross" x1={x(hover)} x2={x(hover)} y1={PAD.t} y2={H - PAD.b} />}
          </svg>
          {hover !== null && (
            <div className="usage-tip" style={{ left: `${(x(hover) / W) * 100}%` }} role="status">
              <strong>{clock(hover)}</strong>
              {series.map((s) => <span key={s.key}><i style={{ background: s.color }} aria-hidden="true" />{s.label}: {tokens(at(s.points, hover))}</span>)}
            </div>
          )}
        </figure>
      )}
      <div className="traffic-table">
        <table>
          <thead><tr><th>Agent</th><th>Input</th><th>Output</th><th>Cached input read</th><th>Context now</th><th>Last report</th></tr></thead>
          <tbody>
            {rows.map((a) => {
              const p = a.points.at(-1), c = a.context;
              return (
                <tr key={a.seat}>
                  <td><Link to={agentPath(rig, a.seat)}>{name(a.seat)}</Link></td>
                  <td>{p ? p.input.toLocaleString() : 'none reported'}</td>
                  <td>{p ? p.output.toLocaleString() : ''}</td>
                  <td>{p ? p.cache_read.toLocaleString() : ''}</td>
                  <td>{c ? `${tokens(c.used_tokens)}${c.window_tokens ? ` of ${tokens(c.window_tokens)} (${Math.round((100 * c.used_tokens) / c.window_tokens)}%)` : ''}${c.estimated ? ' · estimated' : ''}` : 'unknown'}</td>
                  <td>{p ? new Date(p.at_ms).toLocaleString() : ''}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="traffic-note">As each CLI reported it, for this team's current agents (removed agents are not counted). Input excludes cached input. Reports arrive after model calls finish, so work in progress is not counted yet. Long runs are thinned to 240 points per agent; the latest point is always exact.</p>
    </section>
  );
}
