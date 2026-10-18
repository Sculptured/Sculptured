import { api } from '../api';
import { compactNumber, plural } from '../format';
import type { EventRecord } from '../generated/EventRecord';
import type { Meter } from '../generated/Meter';
import type { SeatView } from '../generated/SeatView';
import { usePolled } from '../usePolling';

type Figure = { label: string; value: string; note: string };

/** The latest run's usage reports: their tokens (input and output) and their cost where a source priced it. */
export function runUsage(events: EventRecord[]): { tokens: Figure; cost: Figure } {
  let reports = 0, tokens = 0, finals = 0, priced = 0, usd = 0;
  for (const e of events) {
    if (e.body.type !== 'usage-reported') continue;
    reports += 1;
    tokens += e.body.input_tokens + e.body.output_tokens;
    // A turn's cost rides on its last report; reports counted during the turn never carry one.
    if (e.body.in_turn) continue;
    finals += 1;
    if (e.body.cost) { priced += 1; usd += e.body.cost.usd; }
  }
  const tokenFig = reports === 0
    ? { label: 'Tokens this run', value: 'unknown', note: 'no usage reported yet' }
    : { label: 'Tokens this run', value: compactNumber(tokens), note: '' };
  const costFig = finals === 0
    ? { label: 'Cost', value: 'unknown', note: 'no usage reported yet' }
    : priced === 0
      ? { label: 'Cost', value: 'unknown', note: `${plural(finals, 'report')} had no cost` }
      : { label: 'Cost', value: `${priced < finals ? 'at least ' : ''}$${usd.toFixed(2)}`, note: priced < finals ? `${plural(finals - priced, 'report')} had no cost` : 'estimated from list prices' };
  return { tokens: tokenFig, cost: costFig };
}

/** Without the run's reports: every agent's running total, as its CLI reported it. */
export function seatTokens(seats: Pick<SeatView, 'tokens_in' | 'tokens_out'>[]): Figure {
  const known = seats.filter((s) => s.tokens_in !== null && s.tokens_out !== null);
  if (known.length === 0) return { label: 'Tokens so far', value: 'unknown', note: 'no agent has reported tokens yet' };
  const total = known.reduce((n, s) => n + (s.tokens_in ?? 0) + (s.tokens_out ?? 0), 0);
  const missing = seats.length - known.length;
  return missing === 0
    ? { label: 'Tokens so far', value: compactNumber(total), note: '' }
    : { label: 'Tokens so far', value: `at least ${compactNumber(total)}`, note: `${plural(missing, 'agent')} ${missing === 1 ? 'has' : 'have'} not reported` };
}

/** The run budget's plan share, else its tokens or time: how much of it this run has used. */
function budgetLine(meters: Meter[]): Figure & { meter?: Meter } {
  const m = meters.find((x) => x.unit === 'percent') ?? meters[0];
  if (!m) return { label: 'Run budget', value: 'none set', note: '' };
  if (m.used === null) return { label: 'Run budget', value: 'unknown', note: 'waiting for the first reading' };
  const fmt = (v: number) => (m.unit === 'percent' ? `${Math.round(v)}%` : m.unit === 'minutes' ? `${Math.floor(v)} min` : compactNumber(Math.round(v)));
  return { label: 'Run budget', value: fmt(m.used), note: `of ${fmt(m.hard)} ${m.unit === 'percent' ? 'plan allowed' : 'allowed'}${m.estimated ? ' · includes an estimate' : ''}`, meter: m };
}

function Item({ f, children }: { f: Figure; children?: React.ReactNode }) {
  return (
    <span className="us-item">
      <span className="us-label">{f.label}</span> <strong>{f.value}</strong>
      {f.note && <span className="us-note"> ({f.note})</span>}
      {children}
    </span>
  );
}

/**
 * The Standard look's footer: this run's tokens, its cost and its run budget, in tabular numbers.
 * Every figure comes from the agents' own usage reports and the team budget; anything not
 * reported says unknown, with the reason. Never a guess.
 */
export function UsageStrip({ rig, seats }: { rig: string; seats: SeatView[] }) {
  const usage = usePolled(() => api.runUsage(rig), 30000, [rig]);
  const budget = usePolled(() => api.teamBudget(rig), 10000, [rig]);
  const fromRun = usage.data ? runUsage(usage.data) : null;
  const tokens = fromRun?.tokens ?? seatTokens(seats);
  const cost = fromRun?.cost ?? { label: 'Cost', value: 'unknown', note: usage.error ? "this run's usage reports could not be read" : 'reading usage reports…' };
  const run = budget.error ? { label: 'Run budget', value: 'unknown', note: 'budget status unavailable' } : budget.data ? budgetLine(budget.data.run?.meters ?? []) : { label: 'Run budget', value: '…', note: '' };
  return (
    <footer className="usage-strip" aria-label="Run usage">
      <Item f={tokens} />
      <span className="us-sep" aria-hidden="true">·</span>
      <Item f={cost} />
      <span className="us-sep" aria-hidden="true">·</span>
      <Item f={run}>
        {'meter' in run && run.meter && run.meter.used !== null && (
          <meter className="us-meter" min={0} max={run.meter.hard} value={Math.min(run.meter.used, run.meter.hard)} aria-label={`Run budget: ${run.value} ${run.note}`} />
        )}
      </Item>
    </footer>
  );
}
