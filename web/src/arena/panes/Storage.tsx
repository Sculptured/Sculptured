import { useCallback, useEffect, useState } from 'react';
import { ARENA_GC_WORDS, errorTextWith } from '../../errors';
import { plural } from '../../format';
import { Btn } from '../../shell/ui';
import { arena } from '../../tests/api';
import { HEALTH_CHANGED } from '../../tests/shared';
import type { GcResponse, IntegrationUsage, UsageResponse } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { sizeWords } from '../words';

function useUsage() {
  const [usage, setUsage] = useState<UsageResponse | null>(null);
  const [error, setError] = useState<unknown>(null);
  const load = useCallback(() => {
    arena.usage().then((u) => { setUsage(u); setError(null); }).catch(setError);
  }, []);
  useEffect(load, [load]);
  return { usage, error, load };
}

function Total({ usage }: { usage: UsageResponse }) {
  const share = usage.limit_bytes > 0 ? Math.round((100 * usage.total_bytes) / usage.limit_bytes) : null;
  return (
    <div className="arena-total">
      <p><strong>Saves use {sizeWords(usage.total_bytes)} of {sizeWords(usage.limit_bytes)}</strong>{share != null && <span className="muted"> ({share}%)</span>}</p>
      <meter min={0} max={usage.limit_bytes} value={usage.total_bytes} aria-label="Disk used by saves" />
    </div>
  );
}

function Integration({ row }: { row: IntegrationUsage }) {
  return (
    <li className="arena-item">
      <div className="arena-item-head">
        <strong className="mono">{row.integration}</strong>
        <span className="muted tst-small">used by {row.parts.join(', ')}</span>
        <span className="tst-spacer" />
        {row.error
          ? <span className="tst-status tst-blocked"><span aria-hidden="true">⚠</span>Not answering</span>
          : <span>{sizeWords(row.bytes)}</span>}
      </div>
      {row.error && <p className="tst-small">Its size is unknown and not in the total. It said: <span className="tst-evidence mono" data-raw-evidence>{row.error}</span></p>}
    </li>
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
function CleanUp({ onDone }: { onDone(): void }) {
  const { runs } = useArena();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<GcResponse | null>(null);
  const [error, setError] = useState<unknown>(null);
  const running = !!runs?.some((r) => r.branches.some((b) => b.status === 'running'));
  const run = async () => {
    setBusy(true); setError(null); setResult(null);
    try {
      setResult(await arena.gc());
      window.dispatchEvent(new Event(HEALTH_CHANGED));
      onDone();
    } catch (e) { setError(e); }
    setBusy(false);
  };
  return (
    <section className="arena-cleanup" aria-label="Clean up">
      <div className="arena-row">
        <Btn icon="trash" onClick={run} disabled={busy || running}>{busy ? 'Cleaning up…' : 'Clean up'}</Btn>
        <span className="muted tst-small">{running ? "Can't clean up while a run is going." : 'Removes saves no recorded run uses. Every run keeps its own.'}</span>
      </div>
      {result && <Freed result={result} />}
      <ArenaProblem error={error} text={errorTextWith(ARENA_GC_WORDS, error)} />
    </section>
  );
}

function StorageBody() {
  const { usage, error, load } = useUsage();
  if (error && !usage) return <ArenaProblem error={error} />;
  if (!usage) return <p className="muted">Measuring saves…</p>;
  return (
    <>
      <Total usage={usage} />
      <ul className="arena-list" aria-label="Saves by integration">{usage.integrations.map((r) => <Integration key={r.integration} row={r} />)}</ul>
      <CleanUp onDone={load} />
    </>
  );
}

/** Saves and disk: the total against the limit, each integration's share, and Clean up. */
export function StoragePane() {
  return <div className="arena-pane"><ArenaGate><StorageBody /></ArenaGate></div>;
}
