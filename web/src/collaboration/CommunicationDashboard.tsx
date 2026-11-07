import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import { mergeEvents } from './mergeEvents';
import { usePolled } from '../usePolling';
import type { RigState } from '../useRigStream';
import { nameForId, type NameMap } from '../vocab';
import { agentPath } from '../shell/model';
import { Btn } from '../shell/ui';
import { communicationStats, percentile, trafficBins, type TrafficMessage } from './analytics';
import './analytics.css';
import { CommunicationMap } from './CommunicationMap';
import { DismissibleError } from '../shell/DismissibleError';
import { matchesTraffic, type TrafficFilter } from './trafficFilter';

type Stats = ReturnType<typeof communicationStats>;
type Route = Stats['routes'][number];
type Direction = TrafficFilter['direction'];
type Namer = (id: string) => string;
type Choose = (direction: Direction, from?: string, to?: string) => void;

const size = (n: number) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KiB`);
const delay = (n: number | null) => (n === null ? 'Unknown' : `${n.toLocaleString()} ms`);
const sum = (rows: TrafficMessage[], pick: (m: TrafficMessage) => number) => rows.reduce((n, m) => n + pick(m), 0);
const WINDOW_MS: Record<string, number> = { hour: 3600000, day: 86400000 };

function selectionText(direction: Direction, from: string, to: string, name: Namer) {
  switch (direction) {
    case 'outbound': return `Outbound from ${name(from)}`;
    case 'inbound': return `Inbound to ${name(to)}`;
    case 'one-way': return `${name(from)} → ${name(to)}`;
    case 'two-way': return `${name(from)} ↔ ${name(to)}`;
    default: return '';
  }
}

function messageState(m: TrafficMessage) {
  if (m.failure) return 'Failed';
  if (m.read !== undefined) return 'Read';
  return m.delivered !== undefined ? 'Delivered' : 'Queued / unknown';
}

const gap = (later: number | undefined, earlier: number | undefined) => (later !== undefined && earlier !== undefined && later >= earlier ? later - earlier : null);

/** The filter state lives in the URL so a view can be shared and survives a reload. */
function useTrafficParams(onChosen: () => void) {
  const [params, setParams] = useSearchParams();
  const agent = params.get('trafficAgent') ?? '';
  const window = params.get('trafficWindow') ?? 'all';
  const direction = (params.get('trafficDirection') ?? 'all') as Direction;
  const from = params.get('trafficFrom') ?? '';
  const to = params.get('trafficTo') ?? '';
  const edit = (change: (n: URLSearchParams) => void) => setParams(p => { const n = new URLSearchParams(p); change(n); return n; }, { replace: true });
  const setOrDelete = (n: URLSearchParams, key: string, value: string) => { if (value) n.set(key, value); else n.delete(key); };
  const filter = (key: string, value: string) => { edit(n => setOrDelete(n, key, value)); onChosen(); };
  const choose: Choose = (d, f = '', t = '') => edit(n => {
    n.delete('trafficAgent');
    for (const [k, v] of Object.entries({ trafficDirection: d, trafficFrom: f, trafficTo: t })) setOrDelete(n, k, v);
  });
  return { agent, window, direction, from, to, selection: { direction, from, to } as TrafficFilter, filter, choose };
}

function RouteButtons({ r, name, choose }: { r: { from: string; to: string }; name: Namer; choose: Choose }) {
  return (
    <span className="traffic-endpoints">
      <button aria-label={`Outbound from ${name(r.from)}`} onClick={() => choose('outbound', r.from)}>{name(r.from)}</button>
      <button aria-label={`${name(r.from)} → ${name(r.to)}`} title="Filter this pair" onClick={() => choose('one-way', r.from, r.to)}>→</button>
      <button aria-label={`Inbound to ${name(r.to)}`} onClick={() => choose('inbound', '', r.to)}>{name(r.to)}</button>
    </span>
  );
}

type ToolbarProps = { agent: string; window: string; direction: Direction; ids: string[]; name: Namer; paused: boolean; filter: (k: string, v: string) => void; togglePause: () => void; clear: () => void };

function TrafficToolbar({ agent, window, direction, ids, name, paused, filter, togglePause, clear }: ToolbarProps) {
  return (
    <div className="traffic-toolbar">
      <label>Participant<select value={agent} onChange={e => filter('trafficAgent', e.target.value)}><option value="">Everyone</option>{ids.map(id => <option key={id} value={id}>{name(id)}</option>)}</select></label>
      <label>Window<select value={window} onChange={e => filter('trafficWindow', e.target.value)}><option value="all">Loaded history</option><option value="hour">Last hour</option><option value="day">Last 24 hours</option></select></label>
      <Btn onClick={togglePause}>{paused ? 'Resume updates' : 'Pause updates'}</Btn>
      {direction !== 'all' && <Btn onClick={clear}>Clear communication filter</Btn>}
    </div>
  );
}

function SelectionBar({ label, direction, from, to, choose }: { label: string; direction: Direction; from: string; to: string; choose: Choose }) {
  const pair = direction === 'one-way' || direction === 'two-way';
  return (
    <div className="traffic-selection" role="status"><strong>{label}</strong>{pair && <label>Direction <select aria-label="Communication direction" value={direction} onChange={e => choose(e.target.value as Direction, from, to)}><option value="one-way">One-way →</option><option value="two-way">Both directions ↔</option></select></label>}</div>
  );
}

function Totals({ rows, latency }: { rows: TrafficMessage[]; latency: number[] }) {
  const bytes = sum(rows, m => m.bytes);
  const context = sum(rows, m => m.contextBytes);
  return (
    <dl className="traffic-totals">
      <div><dt>Messages</dt><dd>{rows.length}</dd></div>
      <div><dt>Read / failed</dt><dd>{rows.filter(m => m.read !== undefined).length} / {rows.filter(m => m.failure).length}</dd></div>
      <div><dt>Captured payload</dt><dd>{size(bytes)}</dd></div>
      <div><dt>Architecture context</dt><dd>{size(context)} <small>{rows.length ? Math.round(100 * context / Math.max(1, bytes)) : 0}%</small></dd></div>
      <div><dt>Delivery p50 / p95</dt><dd>{delay(percentile(latency, .5))} / {delay(percentile(latency, .95))}</dd></div>
    </dl>
  );
}

function usageText(u: Stats['usage']) {
  if (!u.reports) return 'No token reports in this window.';
  return `${u.input.toLocaleString()} input · ${u.output.toLocaleString()} output · ${u.cacheRead.toLocaleString()} cache read · ${u.cacheWrite.toLocaleString()} cache write tokens`;
}

type Bin = ReturnType<typeof trafficBins>[number];

function TrafficBars({ rows, bins, start, now, label }: { rows: TrafficMessage[]; bins: Bin[]; start: number; now: number; label: string }) {
  const max = Math.max(1, ...bins.map(b => b.count));
  return <>
    <h3>Message traffic{label ? ` · ${label}` : ''}</h3>
    <div className="traffic-bars" role="img" aria-label={`${rows.length} messages in 30 equal time intervals`} >{bins.map((b, i) => <div key={i} style={{ height: `${100 * b.count / max}%` }} title={`${new Date(b.at).toLocaleString()}: ${b.count} messages, ${size(b.bytes)}`}><span className="sr-only">{new Date(b.at).toLocaleTimeString()}: {b.count}</span></div>)}</div>
    <p className="traffic-note">{new Date(start).toLocaleTimeString()} → {new Date(now).toLocaleTimeString()} · peak {max === 1 && !rows.length ? 0 : max} messages / interval</p>
    <details><summary>Traffic values</summary><ol>{bins.map((b, i) => <li key={i}>{new Date(b.at).toLocaleString()}: {b.count} messages · {size(b.bytes)}</li>)}</ol></details>
  </>;
}

type VisualsProps = { rows: TrafficMessage[]; routes: Route[]; stats: Stats; start: number; now: number; label: string; name: Namer; choose: Choose };

function Visuals({ rows, routes, stats, start, now, label, name, choose }: VisualsProps) {
  const bins = trafficBins(rows, start, now);
  return (
    <div className="traffic-visuals"><section><h3>Who talks to whom</h3><p className="traffic-note">Senders on the left → recipients on the right. Click a name for outbound or inbound messages, or an arrow for the pair. Line width counts messages. Up to 16 names per side; every matching route is listed below.</p><CommunicationMap routes={routes} name={name} onPair={(from, to) => choose('one-way', from, to)} onSender={from => choose('outbound', from)} onRecipient={to => choose('inbound', '', to)} /></section>
      <section><TrafficBars rows={rows} bins={bins} start={start} now={now} label={label} /><h3>Reported model usage</h3><p>{usageText(stats.usage)}</p><p className="traffic-note">Architecture briefing estimate: ~{Math.ceil(sum(rows, m => m.briefing?.length ?? 0) / 4).toLocaleString()} tokens (characters ÷ 4). This is prompt overhead only; it does not measure network bytes, latency cost or reasoning overhead. Delivery latency ends at the runner acknowledgement, not model completion. Usage can arrive late.</p></section></div>
  );
}

function RoutesTable({ routes, orphans, name, choose }: { routes: Route[]; orphans: number; name: Namer; choose: Choose }) {
  return <>
    <h3>Routes · {routes.length}</h3><p className="traffic-note">Fan-in messages appear on each contributing route; route totals can exceed unique message totals. {orphans} receipts have no queued message in this window.</p>
    <div className="traffic-table"><table><thead><tr><th>Sender → recipient</th><th>Messages</th><th>Payload</th><th>Read / failed</th><th>Delivery p95</th></tr></thead><tbody>{routes.map(r => <tr key={r.key}><td><RouteButtons r={r} name={name} choose={choose} /></td><td>{r.count}</td><td>{size(r.bytes)}</td><td>{r.read} / {r.failed}</td><td>{delay(percentile(r.latency, .95))}</td></tr>)}</tbody></table></div>
  </>;
}

function MessageItem({ m, rig, name, choose }: { m: TrafficMessage; rig: string; name: Namer; choose: Choose }) {
  return (
    <li><div className="traffic-message-route">{m.from.map(from => <span key={from}><RouteButtons r={{ from, to: m.to }} name={name} choose={choose} /></span>)}</div>
      <details><summary>{m.from.map(name).join(' + ')} → {name(m.to)} · {messageState(m)} · {new Date(m.queued).toLocaleString()} · {size(m.bytes)}</summary><p>{m.failure}</p><pre>{m.text}</pre>
        <p>Queued → delivery: {delay(gap(m.delivered, m.queued))} · Delivery → read: {delay(gap(m.read, m.delivered))}</p>
        <Link to={`${agentPath(rig, m.to)}?item=${encodeURIComponent(`m-${m.id}`)}`}>Open receiving transcript</Link>
        {m.briefing && <details><summary>Attached architecture context · {size(m.contextBytes)}</summary><pre>{m.briefing}</pre></details>}
        <details><summary>Captured event · {m.event.provenance}</summary><pre>{JSON.stringify(m.event, null, 2)}</pre></details></details></li>
  );
}

const PAGE = 50;

function MessageList({ rows, page, setPage, rig, name, choose }: { rows: TrafficMessage[]; page: number; setPage: (f: (p: number) => number) => void; rig: string; name: Namer; choose: Choose }) {
  return <>
    <h3>Messages · {rows.length}</h3>{!rows.length && <p>No captured messages match this selection. Planned arrows do not count as communication.</p>}
    <ol className="traffic-messages">{rows.slice(page * PAGE, (page + 1) * PAGE).map(m => <MessageItem key={m.id} m={m} rig={rig} name={name} choose={choose} />)}</ol>
    <div className="traffic-toolbar"><Btn disabled={page === 0} onClick={() => setPage(p => p - 1)}>Previous</Btn><span>Page {page + 1}</span><Btn disabled={(page + 1) * PAGE >= rows.length} onClick={() => setPage(p => p + 1)}>Next</Btn></div>
  </>;
}

type Paused = { events: EventRecord[]; at: number } | null;
type Props = { rig: string; state: RigState; connected: boolean; names: NameMap; onClose: () => void };

function useCapture(rig: string, state: RigState, window: string, agent: string) {
  const { data: stored, error, at: updated } = usePolled(() => api.communications(rig, true), 5000, [rig, state.epoch]);
  const [paused, setPaused] = useState<Paused>(null);
  const merged = useMemo(() => mergeEvents(stored ?? [], state.feed).reverse().slice(0, 10000), [stored, state.feed]);
  const events = paused?.events ?? merged;
  const now = paused?.at ?? Date.now();
  const since = now - (WINDOW_MS[window] ?? now);
  const stats = useMemo(() => communicationStats(events, since, agent), [events, since, agent]);
  const togglePause = () => setPaused(paused ? null : { events: merged, at: Date.now() });
  return { error, updated, paused: Boolean(paused), events, now, since, stats, togglePause };
}

export function CommunicationDashboard({ rig, state, connected, names, onClose }: Props) {
  const [page, setPage] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const scroller = useRef<HTMLElement>(null);
  const p = useTrafficParams(() => setPage(0));
  const cap = useCapture(rig, state, p.window, p.agent);
  const { events, now, since, stats } = cap;
  const choose: Choose = (d, f, t) => { p.choose(d, f, t); setPage(0); if (scroller.current) scroller.current.scrollTop = 0; };
  useEffect(() => { heading.current?.focus(); }, []);
  const ids = useMemo(() => [...new Set([...Object.keys(state.seats), ...stats.routes.flatMap(r => [r.from, r.to])])].sort(), [state.seats, stats.routes]);
  const name: Namer = id => (id === 'You' || id === 'System' ? id : nameForId(id, names));
  const rows = stats.rows.filter(m => matchesTraffic(m, p.selection));
  const label = selectionText(p.direction, p.from, p.to, name);
  const routes = stats.routes.filter(r => matchesTraffic({ from: [r.from], to: r.to }, p.selection));
  const start = since || Math.min(now, ...events.map(e => e.at_ms));
  return <section ref={scroller} className="communication-dashboard" aria-label="Communication analytics" onKeyDown={e => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
    <header><h2 ref={heading} tabIndex={-1}>Communications</h2><span className="muted">{cap.paused ? 'Paused' : connected ? 'Live capture' : 'Reconnecting · stale'}</span><Btn onClick={onClose}>Back to board</Btn></header>
    <TrafficToolbar agent={p.agent} window={p.window} direction={p.direction} ids={ids} name={name} paused={cap.paused} filter={p.filter} togglePause={cap.togglePause} clear={() => choose('all')} />
    <p className="traffic-note">Latest {events.length.toLocaleString()} / 10,000 captured messages, handoffs and usage reports for current team members · refreshed every 5s + live updates{cap.updated ? ` · fetched ${new Date(cap.updated).toLocaleTimeString()}` : ''}. {new Date(start).toLocaleString()} – {new Date(now).toLocaleString()}. Older or unmanaged activity may be absent.</p>
    {cap.error && <DismissibleError message={cap.error}>History unavailable; retaining last capture: {cap.error}</DismissibleError>}{(state.gaps > 0 || stats.lost > 0) && <p role="status">Capture gaps reported: {Math.max(state.gaps, stats.lost)} events. Totals are incomplete.</p>}
    {label && <SelectionBar label={label} direction={p.direction} from={p.from} to={p.to} choose={choose} />}<p className="traffic-note">Click a sender for all outbound messages, a recipient for all inbound messages, or → for that pair. Message totals and routes follow this filter; model usage follows the participant and time window.</p>
    <Totals rows={rows} latency={routes.flatMap(r => r.latency)} />
    <Visuals rows={rows} routes={routes} stats={stats} start={start} now={now} label={label} name={name} choose={choose} />
    <RoutesTable routes={routes} orphans={stats.orphanReceipts} name={name} choose={choose} />
    <MessageList rows={rows} page={page} setPage={setPage} rig={rig} name={name} choose={choose} />
  </section>;
}
