import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { plural } from '../../format';
import type { PaneProps } from '../../workspace/registry';
import { arena } from '../../tests/api';
import { branchNames } from '../../tests/model';
import { notComparedLine } from '../../tests/restore';
import { ApproximateMark, StatusPill } from '../../tests/shared';
import type { CompareResponse, RunSummary } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate, ArenaProblem } from '../gate';
import { comparePath } from '../paths';

type Chosen = { a?: string; b?: string };

function BranchSelect({ label, runs, value, onChange }: { label: string; runs: RunSummary[]; value?: string; onChange(id: string): void }) {
  return (
    <label className="tst-field tst-grow">
      <span className="tst-label">{label}</span>
      <select value={value ?? ''} onChange={(e) => onChange(e.target.value)}>
        <option value="">Choose a branch</option>
        {runs.map((r) => {
          const names = branchNames(r.branches);
          return (
            <optgroup key={r.id} label={`Task ${r.task_id}`}>
              {r.branches.map((b) => <option key={b.id} value={b.id}>{names.get(b.id)}</option>)}
            </optgroup>
          );
        })}
      </select>
    </label>
  );
}

function useCompare(a?: string, b?: string) {
  const [cmp, setCmp] = useState<CompareResponse | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    setCmp(null); setError(null);
    if (!a || !b || a === b) return;
    let alive = true;
    arena.compare(a, b).then((c) => { if (alive) setCmp(c); }).catch((e) => { if (alive) setError(e); });
    return () => { alive = false; };
  }, [a, b]);
  return { cmp, error };
}

function Summary({ cmp, a, b }: { cmp: CompareResponse; a: string; b: string }) {
  const rows = cmp.differing_rows.length;
  return (
    <div className="arena-compare" role="group" aria-label="Comparison">
      <p className="arena-row"><StatusPill status={cmp.a.status} short /><ApproximateMark of={cmp.a} /><span aria-hidden="true">→</span><StatusPill status={cmp.b.status} short /><ApproximateMark of={cmp.b} /></p>
      <p>{cmp.first_divergent_seq != null ? `They first differ at step ${cmp.first_divergent_seq}.` : 'They take the same steps.'}{' '}
        {rows ? `In the final world, ${plural(rows, 'row')} ${rows === 1 ? 'differs' : 'differ'}.` : 'No row of the final world differs.'}</p>
      {notComparedLine(cmp.not_compared, 'compare') && <p className="tst-small">{notComparedLine(cmp.not_compared, 'compare')}</p>}
      <Link to={comparePath(a, b)}>Open the full comparison</Link>
    </div>
  );
}

function CompareBody({ settings, setSettings }: PaneProps<Chosen>) {
  const { runs: loaded } = useArena();
  const runs = loaded.value;
  const { a, b } = settings;
  const { cmp, error } = useCompare(a, b);
  if (!runs) return loaded.error ? <ArenaProblem error={loaded.error} /> : <p className="muted">Reading runs…</p>;
  return (
    <>
      <ArenaProblem error={loaded.error} />
      <div className="tst-row">
        <BranchSelect label="First" runs={runs} value={a} onChange={(v) => setSettings({ ...settings, a: v || undefined })} />
        <BranchSelect label="Second" runs={runs} value={b} onChange={(v) => setSettings({ ...settings, b: v || undefined })} />
      </div>
      {(!a || !b) && <p className="muted">Choose two branches to compare.</p>}
      {a && b && a === b && <p className="muted">Choose two different branches.</p>}
      <ArenaProblem error={error} />
      {cmp && a && b && <Summary cmp={cmp} a={a} b={b} />}
    </>
  );
}

/** Any two branches: where they first differ and how their final worlds differ. */
export function ComparePane(props: PaneProps) {
  return <div className="arena-pane"><ArenaGate><CompareBody {...(props as PaneProps<Chosen>)} /></ArenaGate></div>;
}
