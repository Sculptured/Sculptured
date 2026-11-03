import { useState, type CSSProperties } from 'react';
import { ARENA_GC_WORDS, errorTextWith } from '../../errors';
import { plural } from '../../format';
import { Btn } from '../../shell/ui';
import { arena } from '../../tests/api';
import type { GcResponse, UsageResponse } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { sizeWords } from '../words';

const SERIES = 6;
const seg = (i: number) => ({ '--seg': `var(--series-${(i % SERIES) + 1})` }) as CSSProperties;
const pct = (n: number, of: number) => `${Math.min(100, (100 * n) / of)}%`;

/** Saves against the limit: one bar split by integration, with a tick at the limit. */
function UsageBar({ usage }: { usage: UsageResponse }) {
  const scale = Math.max(usage.total_bytes, usage.limit_bytes, 1);
  const share = usage.limit_bytes > 0 ? Math.round((100 * usage.total_bytes) / usage.limit_bytes) : null;
  return (
    <div className="arena-total">
      <p className="arena-row">
        <strong>Saves use {sizeWords(usage.total_bytes)} of {sizeWords(usage.limit_bytes)}</strong>
        {share != null && <span className={`arena-num${share >= 90 ? ' arena-warn' : ' muted'}`}>{share >= 90 && <span aria-hidden="true">⚠ </span>}{share}%</span>}
      </p>
      <div className="arena-stack" role="img" aria-label={`Saves use ${share ?? '?'}% of the limit`}>
        {usage.integrations.map((r, i) => (r.bytes > 0 ? <span key={r.integration} className="arena-seg" style={{ ...seg(i), width: pct(r.bytes, scale) }} title={`${r.integration}: ${sizeWords(r.bytes)}`} /> : null))}
        <span className="arena-limit" style={{ left: pct(usage.limit_bytes, scale) }} title={`Limit ${sizeWords(usage.limit_bytes)}`} />
      </div>
      <p className="arena-limit-note"><span aria-hidden="true">▏</span>limit {sizeWords(usage.limit_bytes)}</p>
    </div>
  );
}

/** Each integration: its colour in the bar, the parts that use it, and its bytes (or why they're unknown). */
function Legend({ usage }: { usage: UsageResponse }) {
  return (
    <table className="arena-table" aria-label="Saves by integration">
      <thead><tr><th>Integration</th><th>Used by</th><th className="arena-num">Size</th></tr></thead>
      <tbody>
        {usage.integrations.map((r, i) => (
          <tr key={r.integration}>
            <td><span className="arena-swatch" style={seg(i)} aria-hidden="true" /> <span className="mono">{r.integration}</span></td>
            <td className="muted">{r.parts.join(', ')}</td>
            <td className="arena-num">
              {r.error
                ? <><span className="tst-status tst-blocked"><span aria-hidden="true">⚠</span>Not answering</span> <span className="tst-evidence mono tst-small" data-raw-evidence>{r.error}</span></>
                : sizeWords(r.bytes)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Freed({ result }: { result: GcResponse }) {
  return (
    <div className="tst-note" role="status">
      <span>Freed {sizeWords(result.freed_bytes)}. Removed {plural(result.removed.length, 'save')}.</span>
      {result.removed.length > 0 && <span className="tst-evidence mono tst-small" data-raw-evidence>{result.removed.join(', ')}</span>}
      {result.errors.map((e) => (
        <span key={e.integration} className="tst-small">{e.integration} could not clean up. It said: <span className="mono" data-raw-evidence>{e.message}</span></span>
      ))}
    </div>
  );
}

/** "Clean up": runs Arena's cleanup and says what it freed. Not while a branch is running. */
function CleanUp() {
  const { runs, cleanedUp } = useArena();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<GcResponse | null>(null);
  const [error, setError] = useState<unknown>(null);
  const running = !!runs.value?.some((r) => r.branches.some((b) => b.status === 'running'));
  const run = async () => {
    setBusy(true); setError(null); setResult(null);
    try {
      const r = await arena.gc();
      setResult(r);
      cleanedUp({ at: Date.now(), freed_bytes: r.freed_bytes });
    } catch (e) { setError(e); }
    setBusy(false);
  };
  return (
    <section className="arena-cleanup" aria-label="Clean up">
      <div className="arena-row">
        <Btn size="sm" icon="trash" onClick={run} disabled={busy || running}>{busy ? 'Cleaning up…' : 'Clean up'}</Btn>
        <span className="muted tst-small">{running ? "Can't clean up while a run is going." : 'Removes saves no recorded run uses. Every run keeps its own.'}</span>
      </div>
      {result && <Freed result={result} />}
      <ArenaProblem error={error} text={errorTextWith(ARENA_GC_WORDS, error)} />
    </section>
  );
}

/** A failed re-read shows above the last answer, never silently stale. */
function StorageBody() {
  const { usage } = useArena();
  return (
    <>
      <ArenaProblem error={usage.error} />
      {!usage.value && !usage.error && <p className="muted">Measuring saves…</p>}
      {usage.value && <UsageBar usage={usage.value} />}
      {usage.value && <Legend usage={usage.value} />}
      {usage.value && <CleanUp />}
    </>
  );
}

/** Saves and disk: the total against the limit, each integration's share, and Clean up. */
export function StoragePane() {
  return <div className="arena-pane"><ArenaGate><StorageBody /></ArenaGate></div>;
}
