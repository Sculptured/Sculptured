import { useEffect, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { arena } from './api';
import { branchNames, costSource, diffHunks, dollars, jsonDiff } from './model';
import { runPath } from './paths';
import { APPROXIMATE_NOTE, isApproximate, notComparedLine } from './restore';
import { ApproximateMark, ErrorNote, StatusPill, pretty } from './shared';
import { KIND } from './Swimlanes';
import type { BranchDetail, CompareResponse, CompareSide, Event, RowDiff } from './types';

/** "−62%" / "+8%" from a to b, or nothing when either side is missing. */
function delta(a: number | null, b: number | null): { text: string; better: boolean } | null {
  if (a == null || b == null || a === 0) return null;
  const d = Math.round((100 * (b - a)) / a);
  return { text: `${d > 0 ? '+' : d < 0 ? '−' : '±'}${Math.abs(d)}%`, better: d < 0 };
}

function Metric({ label, a, b, fmt, lowerIsBetter = true }: { label: string; a: number | null; b: number | null; fmt: (v: number) => string; lowerIsBetter?: boolean }) {
  const d = delta(a, b);
  return (
    <div className="tst-metric">
      <span className="tst-label">{label}</span>
      <span className="tst-metric-a">{a != null ? fmt(a) : '—'}</span>
      <span className="tst-metric-b">{b != null ? fmt(b) : '—'}</span>
      <span className={`tst-delta${d ? (d.better === lowerIsBetter ? ' good' : ' bad') : ''}`}>{d?.text ?? ''}</span>
    </div>
  );
}

function stepLine(ev: Event | undefined) {
  if (!ev) return <span className="muted">No step here: this branch had already ended.</span>;
  const p = (ev.payload ?? {}) as Record<string, unknown>;
  const kind = ev.kind === 'tool_call' ? ((Array.isArray(p.writes) && p.writes.length && p.error == null) ? 'write' : 'read')
    : ev.kind === 'model_call' ? 'model' : ev.kind === 'handoff' ? 'handoff' : ev.kind === 'check' ? 'check' : ev.kind === 'agent_end' ? 'end' : 'start';
  return (
    <>
      <div><span aria-hidden="true">{KIND[kind].glyph}</span> <strong className="mono">{typeof p.name === 'string' ? p.name : KIND[kind].word}</strong> <span className="muted tst-small">step {ev.seq} · {ev.agent ?? 'check'}</span></div>
      {ev.kind === 'agent_end' ? <p className="muted">{p.ok ? 'Finished' : 'Stopped with an error'}{typeof p.tokens === 'number' ? ` · ${p.tokens.toLocaleString()} tokens` : ''}{typeof p.seconds === 'number' ? ` · ${p.seconds.toFixed(1)} s` : ''}</p>
        : ev.kind === 'check' ? <p className="muted">{p.passed ? '✓ The final database matches what the task expects.' : '✕ The final database does not match what the task expects.'}</p>
        : <pre className="tst-json">{pretty(ev.kind === 'tool_call' ? p.args : ev.kind === 'handoff' ? p.note : ev.kind === 'model_call' ? (p.response as { choices?: { message?: unknown }[] } | undefined)?.choices?.[0]?.message ?? p.response : p)}</pre>}
    </>
  );
}

const FIELD: Record<string, string> = { prompt: 'instructions', model: 'model', kind: 'agent' };

/** What a rerun changed, per agent, e.g. "Resolver: instructions". */
function Changes({ br }: { br: BranchDetail }) {
  const entries = Object.entries(br.changes ?? {});
  if (br.parent_id == null) return <span className="muted tst-small">The first run</span>;
  if (!entries.length) return <span className="muted tst-small">Rerun from step {br.start_seq} with nothing changed</span>;
  const name = (id: string) => br.team.agents.find((a) => a.id === id)?.name ?? id;
  return (
    <span className="tst-small">
      Rerun from step {br.start_seq}, changed:{' '}
      {entries.map(([agent, c], i) => (
        <span key={agent}>{i > 0 ? '; ' : ''}<strong>{name(agent)}</strong> {Object.keys(c).filter((f) => (c as Record<string, unknown>)[f] != null).map((f) => FIELD[f] ?? f).join(', ')}</span>
      ))}
    </span>
  );
}

/** "Final database: 2 rows differ"; never "identical" when a part was left out. */
function rowsTitle(cmp: CompareResponse): string {
  const n = cmp.differing_rows.length;
  if (n) return `Final database: ${n} row${n === 1 ? '' : 's'} differ`;
  return cmp.not_compared?.length ? 'Final database: no differences in the parts compared' : 'Final database: identical';
}

function RowDiffView({ r, nameA, nameB }: { r: RowDiff; nameA: string; nameB: string }) {
  return (
    <div className="tst-rowdiff">
      <div className="mono tst-small">{r.key}{!r.a && <span className="muted"> (only in {nameB})</span>}{!r.b && <span className="muted"> (only in {nameA})</span>}</div>
      <pre className="tst-json tst-diff">
        {diffHunks(jsonDiff(r.a ?? undefined, r.b ?? undefined)).map((l, i) => l == null
          ? <div key={i} className="dl-gap">⋯</div>
          : <div key={i} className={l.op === '+' ? 'dl-add' : l.op === '-' ? 'dl-del' : 'dl-same'}>{l.op} {l.text}</div>)}
      </pre>
    </div>
  );
}

/** `/tests/compare/:a/:b`: two branches of a run side by side. */
export function ComparePage() {
  const { a = '', b = '' } = useParams();
  const [data, setData] = useState<{ cmp: CompareResponse; x: BranchDetail; y: BranchDetail; names: Map<string, string>; run: string } | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    setData(null); setError(null);
    Promise.all([arena.compare(a, b), arena.branch(a), arena.branch(b), arena.runs()])
      .then(([cmp, x, y, runs]) => {
        if (!alive) return;
        const run = runs.find((r) => r.branches.some((br) => br.id === a));
        setData({ cmp, x, y, names: branchNames(run?.branches ?? [x, y]), run: run?.id ?? x.run_id });
      })
      .catch((e) => { if (alive) setError(e); });
    return () => { alive = false; };
  }, [a, b]);

  if (!data) {
    return <main className="page page-wide tst-page"><Crumbs />{error ? <ErrorNote error={error} /> : <p className="muted">Comparing…</p>}</main>;
  }
  const { cmp, x, y, names, run } = data;
  const name = (br: BranchDetail) => names.get(br.id) ?? br.id.slice(0, 8);
  const side = (br: BranchDetail, s: CompareSide) => (
    <Link className="tst-card tst-cmp-side" to={runPath(run, br.id)}>
      <span className="tst-cmp-name">{name(br)}</span>
      <StatusPill status={s.status} />
      <ApproximateMark of={s} />
      <Changes br={br} />
      {br.fork_note && <span className="muted tst-small">{br.fork_note}</span>}
    </Link>
  );
  const div = cmp.first_divergent_seq;
  return (
    <main className="page page-wide tst-page">
      <Crumbs />
      <h1 className="page-title">{name(x)} vs {name(y)}</h1>
      <div className="tst-cmp-sides">{side(x, cmp.a)}<span className="tst-vs" aria-hidden="true">→</span>{side(y, cmp.b)}</div>
      {(isApproximate(cmp.a) || isApproximate(cmp.b)) && <div className="tst-note">{APPROXIMATE_NOTE}</div>}
      <section className="tst-card tst-cmp-metrics" aria-label="Totals">
        <div className="tst-metric tst-metric-head"><span /><span>{name(x)}</span><span>{name(y)}</span><span>Change</span></div>
        <Metric label="Time" a={cmp.a.seconds} b={cmp.b.seconds} fmt={(v) => `${v.toFixed(1)} s`} />
        <Metric label="Tokens" a={cmp.a.tokens} b={cmp.b.tokens} fmt={(v) => v.toLocaleString()} />
        <Metric label="Cost" a={cmp.a.cost_usd} b={cmp.b.cost_usd} fmt={dollars} />
        <p className="muted tst-small">
          Each branch counts only the steps it ran itself.{' '}
          {[cmp.a, cmp.b].some((s) => s.cost_usd == null) ? 'A side without a dollar figure reports tokens only.' : `Cost: ${costSource(cmp.a.cost_source) || 'unknown source'}${cmp.a.cost_source !== cmp.b.cost_source ? ` vs ${costSource(cmp.b.cost_source) || 'unknown source'}` : ''}.`}
        </p>
      </section>
      <section className="tst-card" aria-label="Where they differ">
        <h2 className="tst-h2">{div != null ? `They first differ at step ${div}` : 'They take the same steps'}</h2>
        {div != null && (
          <div className="tst-cmp-cols">
            <div><div className="tst-label">{name(x)}</div>{stepLine(x.events.find((e) => e.seq === div))}</div>
            <div><div className="tst-label">{name(y)}</div>{stepLine(y.events.find((e) => e.seq === div))}</div>
          </div>
        )}
      </section>
      <section className="tst-card" aria-label="Database differences">
        <h2 className="tst-h2">{rowsTitle(cmp)}</h2>
        {notComparedLine(cmp.not_compared, 'compare') && <p className="tst-small">{notComparedLine(cmp.not_compared, 'compare')}</p>}
        {cmp.differing_rows.map((r) => <RowDiffView key={r.key} r={r} nameA={name(x)} nameB={name(y)} />)}
      </section>
    </main>
  );
}

function Crumbs() {
  return <nav className="tst-crumbs" aria-label="Breadcrumb"><Link to="/tests">Tests</Link><span aria-hidden="true">/</span><span>Compare</span></nav>;
}
