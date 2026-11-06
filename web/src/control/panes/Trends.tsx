import { compactNumber } from '../../format';
import type { PaneProps } from '../../workspace/registry';
import { readingTime, useControl } from '../context';
import { EMPTY_PLANT } from '../plant/model';
import { KindBadge } from '../plant/parts';
import { contextSeries, duration, latency, ratesBySystem, totals } from '../plant/trends';
import { Spark } from './Spark';

/**
 * Trends: actions per minute for each system (with its reads, writes and abnormal calls), the context
 * each model call carried, tokens and cost summed from the reports, and how long actions took. A
 * reading nobody reported says so; it is never shown as zero.
 */

function BySystem() {
  const { source, now } = useControl();
  const plant = source.plant ?? EMPTY_PLANT;
  const rates = ratesBySystem(plant.systems, plant.actions, readingTime(source, now), source.historySince);
  if (rates.length === 0) return <p className="ctl-empty">Actions per system show here once agents use tools.</p>;
  return (
    <table className="ctl-table" aria-label="Actions per minute by system">
      <thead><tr><th scope="col">System</th><th scope="col">Last 15 min</th><th scope="col" className="ctl-num">Now /min</th><th scope="col" className="ctl-num">R</th><th scope="col" className="ctl-num">W</th><th scope="col" className="ctl-num">⊘ ✕</th></tr></thead>
      <tbody>
        {rates.map((r) => (
          <tr key={r.system.id} className={r.abnormal ? 'is-abnormal' : undefined}>
            <td><KindBadge s={r.system} /> {r.system.name}</td>
            <td><Spark values={r.perMin} label={`${r.system.name}: actions per minute, last ${r.perMin.length} minutes: ${r.perMin.map((x) => x ?? 'unknown').join(', ')}`} /></td>
            <td className="ctl-num">{r.perMin.at(-1) ?? 'unknown'}</td>
            <td className="ctl-num">{r.reads}</td>
            <td className="ctl-num">{r.writes}</td>
            <td className={`ctl-num${r.abnormal ? ' pl-abn' : ''}`}>{r.abnormal}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Reading({ label, value, note, values, spark }: { label: string; value: string; note?: string; values?: number[]; spark?: string }) {
  return (
    <div className="ctl-reading">
      <span className="ctl-reading-label">{label}</span>
      <span className={`ctl-reading-value${value.startsWith('not ') ? ' is-unknown' : ''}`}>{value}</span>
      {values && values.length > 1 && <Spark values={values} label={spark ?? label} />}
      {note && <span className="ctl-reading-note">{note}</span>}
    </div>
  );
}

function Usage() {
  const { source } = useControl();
  const plant = source.plant ?? EMPTY_PLANT;
  const ctx = contextSeries(plant.usage);
  const t = totals(plant.usage);
  const lat = latency(plant.actions);
  return (
    <div className="ctl-readings" role="group" aria-label="Context, tokens, cost and latency">
      <Reading label="Context per call" value={ctx.values.length ? compactNumber(ctx.values.at(-1)!) : 'not reported'} note={ctx.peak !== undefined ? `peak ${compactNumber(ctx.peak)} tokens` : undefined}
        values={ctx.values} spark={`Context per call, last ${ctx.values.length} calls`} />
      <Reading label="Tokens" value={t.tokens !== undefined ? compactNumber(t.tokens) : 'not reported'} note="summed from usage reports" />
      <Reading label="Cost" value={t.costUsd !== undefined ? `$${t.costUsd.toFixed(2)}` : 'not reported'} note={t.costUsd !== undefined && !t.priced ? 'at least: some reports had no price' : 'estimated'} />
      <Reading label="Latency" value={lat.median !== undefined ? duration(lat.median) : 'not recorded'} note={lat.p90 !== undefined ? `median · 90% under ${duration(lat.p90)} (${lat.count} calls)` : source.kind === 'simulated' ? 'No call duration was recorded in this run' : undefined}
        values={lat.recent} spark="Latency of the latest calls" />
    </div>
  );
}

export function TrendsPane(_: PaneProps) {
  return (
    <div className="ctl-metrics pl-trends">
      <h3 className="ctl-sub">Actions per minute by system</h3>
      <BySystem />
      <h3 className="ctl-sub">Context, tokens, cost, latency</h3>
      <Usage />
    </div>
  );
}
