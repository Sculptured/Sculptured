import { compactNumber } from '../format';
import type { Meter } from '../generated/Meter';
import type { RunStatus } from '../generated/RunStatus';
import { nameForId, type NameMap } from '../vocab';
import { api } from '../api';
import { PausedRun } from './PausedRun';
import { RaiseLimit, type LimitUnit } from './RaiseLimit';

function amount(m: Meter, v: number): string {
  if (m.unit === 'percent') return `${+v.toFixed(1)}%`;
  if (m.unit === 'minutes') return `${Math.floor(v)} min`;
  return compactNumber(Math.round(v));
}

function age(ms: number | null, now: number): string {
  if (ms === null) return '';
  const s = Math.max(0, Math.round((now - ms) / 1000));
  return s < 60 ? `reading ${s} s ago` : `reading ${Math.round(s / 60)} min ago`;
}

/** One budget, in words and as a bar: how much is used, where wrap-up and the stop come. */
/** `raise`: the run's team and whether the run stopped or wraps up, when the limit can be raised here. */
export function MeterRow({ m, now, raise }: { m: Meter; now: number; raise?: { rig: string; continues: boolean } }) {
  if (m.used === null) {
    return <div className="run-meter"><span>{m.what}</span><span className="muted">waiting for the first plan reading</span><span /></div>;
  }
  const evidence = [m.estimated ? 'includes an estimate' : '', age(m.observed_ms, now)].filter(Boolean).join(' · ');
  // A plan budget is the run's own share of the window, not the window's level (which includes other use).
  const words = m.unit === 'percent'
    ? `${m.what}: this run used ${amount(m, m.used)} of the window · it may use ${amount(m, m.hard)}, wraps up at ${amount(m, m.soft)}`
    : `${m.what}: ${amount(m, m.used)} of ${amount(m, m.hard)} · wrap-up at ${amount(m, m.soft)}`;
  return (
    <div className="run-meter">
      <span>{words}</span>
      <meter min={0} max={m.hard} low={m.soft} high={m.soft} optimum={0} value={Math.min(m.used, m.hard)} aria-label={words} />
      <span className="muted">{evidence}</span>
      {raise && m.limit && <RaiseLimit label={m.what} unit={m.unit as LimitUnit} hard={m.hard} continues={raise.continues}
        send={(hard) => api.raiseLimit(raise.rig, { limit: m.limit, hard, provider: m.provider, window: m.window })} />}
    </div>
  );
}

/** The run's budgets on the Run board, what the run did at a soft or hard value, and who waits for a slot. */
export function RunMeters({ rig, run, waiting, names, now = Date.now() }: { rig: string; run: RunStatus; waiting: string[]; names: NameMap; now?: number }) {
  const s = run.state;
  return (
    <section className="run-meters" aria-label="Run budget">
      {s.state === 'paused' && <PausedRun rig={rig} s={s} mode={run.continue_with} />}
      {s.state !== 'normal' && s.state !== 'paused' && (
        <p className={`run-budget-state ${s.state === 'stopped' ? 'tone-red' : 'tone-amber'}`} role="status">
          {s.state === 'stopped' ? 'Stopped by the run budget: ' : 'Wrapping up: '}{s.reason}
        </p>
      )}
      {run.meters.map((m) => <MeterRow key={m.what} m={m} now={now} raise={{ rig, continues: s.state === 'stopped' || s.state === 'wrap-up' }} />)}
      {waiting.length > 0 && <p className="muted">Waiting for a slot: {waiting.map((w) => nameForId(w, names)).join(', ')}</p>}
    </section>
  );
}
