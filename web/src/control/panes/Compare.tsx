import { useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { ArenaProblem } from '../../arena/gate';
import { comparePath } from '../../arena/paths';
import { branchTree } from '../../tests/model';
import { APPROXIMATE_NOTE, isApproximate } from '../../tests/restore';
import { Differences, Totals } from '../../tests/ComparePage';
import { ApproximateMark, StatusPill } from '../../tests/shared';
import type { Comparison } from '../../tests/types';
import type { PaneProps } from '../../workspace/registry';
import { useControl } from '../context';
import type { SimulatedRun } from '../source';

/**
 * Compare: another branch of the run (its parent unless the operator chose another) against this
 * one, with the comparison page's totals and differences: the change column is what this branch
 * changed. Both must have finished.
 */

type CompareSettings = { other?: string };

/**
 * The branch to compare with: the chosen one if it is in this run, else this branch's parent, else
 * its newest fork that has finished.
 */
function otherOf(a: SimulatedRun, chosen?: string): string | undefined {
  const b = a.branch!;
  if (chosen && chosen !== b.id && a.branches.some((x) => x.id === chosen)) return chosen;
  if (b.parent_id) return b.parent_id;
  const forks = a.branches.filter((x) => x.parent_id === b.id && x.status !== 'running');
  return forks.sort((x, y) => y.created_at.localeCompare(x.created_at))[0]?.id;
}

type Read = { state: 'idle' } | { state: 'loading' } | { state: 'ready'; d: Comparison } | { state: 'failed'; error: unknown };

function useComparison(a: SimulatedRun, other: string | undefined, ready: boolean) {
  const [read, setRead] = useState<Read>({ state: 'idle' });
  const load = useRef(a.compare);
  useEffect(() => { load.current = a.compare; });
  useEffect(() => {
    if (!other || !ready) { setRead({ state: 'idle' }); return; }
    let alive = true;
    setRead({ state: 'loading' });
    load.current(other).then((d) => { if (alive) setRead({ state: 'ready', d }); }).catch((error: unknown) => { if (alive) setRead({ state: 'failed', error }); });
    return () => { alive = false; };
  }, [other, ready, a.branchId]);
  return read;
}

function Choose({ a, other, onChange }: { a: SimulatedRun; other?: string; onChange(id: string | undefined): void }) {
  const here = a.branch!.id;
  return (
    <label className="ctl-field">Compare with
      <select value={other ?? ''} onChange={(e) => onChange(e.target.value || undefined)}>
        <option value="">Choose a branch</option>
        {branchTree(a.branches).filter(({ branch: b }) => b.id !== here).map(({ branch: b, depth }) => (
          <option key={b.id} value={b.id} disabled={b.status === 'running'}>{'  '.repeat(depth)}{a.names.get(b.id) ?? b.id}{b.status === 'running' ? ' (running)' : ''}</option>
        ))}
      </select>
    </label>
  );
}

function Result({ d, a, other }: { d: Comparison; a: SimulatedRun; other: string }) {
  const name = (id: string) => a.names.get(id) ?? id.slice(0, 8);
  return (
    <div className="ctl-compare-body">
      <p className="ctl-compare-sides">
        <b>{name(d.x.id)}</b> <StatusPill status={d.cmp.a.status} short /> <ApproximateMark of={d.cmp.a} />
        <span aria-hidden="true"> → </span>
        <b>{name(d.y.id)}</b> <StatusPill status={d.cmp.b.status} short /> <ApproximateMark of={d.cmp.b} />
        <Link className="ctl-link" to={comparePath(other, a.branchId)}>Open the full comparison</Link>
      </p>
      {(isApproximate(d.cmp.a) || isApproximate(d.cmp.b)) && <p className="ctl-note">{APPROXIMATE_NOTE}</p>}
      <Totals d={d} />
      <Differences d={d} />
    </div>
  );
}

export function ComparePane({ settings, setSettings }: PaneProps) {
  const s = settings as CompareSettings;
  const a = useControl().source.arena!;
  const b = a.branch!;
  const other = otherOf(a, s.other);
  const otherRunning = a.branches.find((x) => x.id === other)?.status === 'running';
  const ready = b.status !== 'running' && !otherRunning;
  const read = useComparison(a, other, ready);
  return (
    <div className="ctl-compare">
      <Choose a={a} other={other} onChange={(id) => setSettings({ ...s, other: id })} />
      {!other && <p className="ctl-empty">{a.branches.length > 1 ? 'Choose a branch of this run to compare with.' : 'This run has one branch. Fork from a step in the Timeline to have something to compare.'}</p>}
      {other && !ready && <p className="ctl-empty">◌ Both branches must finish before they can be compared.</p>}
      {read.state === 'loading' && <p className="ctl-empty">Comparing…</p>}
      {read.state === 'failed' && <div className="ctl-empty"><ArenaProblem error={read.error} /></div>}
      {read.state === 'ready' && other && <Result d={read.d} a={a} other={other} />}
    </div>
  );
}
