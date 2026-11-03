import type { ReactNode } from 'react';
import { api } from '../../api';
import { compactNumber } from '../../format';
import type { MachineSample } from '../../generated/MachineSample';
import { usePolled } from '../../usePolling';
import { useControl } from '../context';
import { Spark } from './Spark';

/**
 * Metrics: tokens and cost as reported, tool calls per minute counted from events, each agent's
 * tokens, and this machine's load. Anything not reported says "unknown".
 */

const MACHINE_MS = 10_000;
const UNKNOWN = 'unknown';

function Reading({ label, value, note, children }: { label: string; value: string; note?: string; children?: ReactNode }) {
  return (
    <div className="ctl-reading">
      <span className="ctl-reading-label">{label}</span>
      <span className={`ctl-reading-value${value === UNKNOWN ? ' is-unknown' : ''}`}>{value}</span>
      {children}
      {note && <span className="ctl-reading-note">{note}</span>}
    </div>
  );
}

function TeamReadings() {
  const { source } = useControl();
  const m = source.metrics;
  const rate = m.toolCallsPerMin;
  const last = rate.at(-1);
  return (
    <div className="ctl-readings" role="group" aria-label="Team">
      {m.tokensIn === undefined && m.tokens !== undefined
        ? <Reading label="Tokens" value={compactNumber(m.tokens)} note="in and out, as each agent reported at its end" />
        : <>
          <Reading label="Tokens in" value={m.tokensIn !== undefined ? compactNumber(m.tokensIn) : UNKNOWN} note="as reported" />
          <Reading label="Tokens out" value={m.tokensOut !== undefined ? compactNumber(m.tokensOut) : UNKNOWN} note="as reported" />
        </>}
      <Reading label="Cost" value={m.costUsd !== undefined ? `$${m.costUsd.toFixed(2)}` : UNKNOWN} note={m.costUsd !== undefined ? 'estimated, latest run' : 'not every report was priced'} />
      <Reading label="Tool calls / min" value={last !== undefined ? String(last) : UNKNOWN} note={`this minute · ${rate.reduce((n, x) => n + x, 0)} in the last ${rate.length} min`}>
        <Spark values={rate} label={`Tool calls per minute over the last ${rate.length} minutes: ${rate.join(', ')}`} />
      </Reading>
    </div>
  );
}

function AgentTokens() {
  const { source } = useControl();
  if (source.agents.length === 0) return null;
  return (
    <table className="ctl-table" aria-label="Tokens by agent">
      <thead><tr><th scope="col">Agent</th><th scope="col">Status</th><th scope="col" className="ctl-num">In</th><th scope="col" className="ctl-num">Out</th></tr></thead>
      <tbody>
        {source.agents.map((a) => (
          <tr key={a.id}>
            <td>{a.name}</td>
            <td><span className={`ctl-glyph tone-${a.status.tone}`} aria-hidden="true">{a.status.glyph}</span> {a.status.word}</td>
            <td className="ctl-num">{a.tokens ? compactNumber(a.tokens.in) : UNKNOWN}</td>
            <td className="ctl-num">{a.tokens ? compactNumber(a.tokens.out) : UNKNOWN}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const gb = (n: number) => `${(n / 2 ** 30).toFixed(1)} GB`;

function Machine() {
  const { data, error } = usePolled<MachineSample[]>(() => api.machineMetrics(), MACHINE_MS, []);
  const last = data?.at(-1);
  if (!last) return <p className="ctl-note">This machine: {error ? `unknown (could not load: ${error})` : 'loading…'}</p>;
  const cpu = (data ?? []).slice(-30).map((s) => Math.round(s.cpu_percent ?? 0));
  return (
    <div className="ctl-readings" role="group" aria-label="This machine">
      <Reading label="CPU" value={last.cpu_percent !== null ? `${Math.round(last.cpu_percent)}%` : UNKNOWN}>
        {last.cpu_percent !== null && <Spark values={cpu} label={`CPU use over the last ${cpu.length} samples`} />}
      </Reading>
      <Reading label="Memory" value={gb(last.memory_used)} note={`of ${gb(last.memory_total)}`} />
      <Reading label="Disk free" value={gb(last.disk_available)} note={`of ${gb(last.disk_total)}`} />
    </div>
  );
}

export function MetricsPane() {
  return (
    <div className="ctl-metrics">
      <h3 className="ctl-sub">Team</h3>
      <TeamReadings />
      <AgentTokens />
      <h3 className="ctl-sub">This machine</h3>
      <Machine />
    </div>
  );
}
