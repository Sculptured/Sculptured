import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { Btn, Icon, Segmented } from '../shell/ui';
import { arena, follow } from './api';
import { branchNames, branchTree, bySteps, costSource, dollars, inOrder, layout, summarize } from './model';
import { comparePath, runPath } from './paths';
import { RerunPanel } from './RerunPanel';
import { worldLine } from './restore';
import { ApproximateMark, ErrorNote, Stat, StatusPill, useArenaHealth } from './shared';
import { StepInspector } from './StepInspector';
import { Legend, StepList, Swimlanes, type Scale } from './Swimlanes';
import type { Branch, BranchDetail, Event } from './types';

type View = 'lanes' | 'list';

/** `/tests/runs/:run/:branch`: one branch of a run, step by step, with Change & rerun from any step. */
export function RunPage() {
  const { run = '', branch: branchId = '' } = useParams();
  const nav = useNavigate();
  const loc = useLocation();
  const [sp, setSp] = useSearchParams();
  const { health } = useArenaHealth();
  const [detail, setDetail] = useState<BranchDetail | null>(null);
  const [siblings, setSiblings] = useState<Branch[]>([]);
  const [reason, setReason] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [rerunning, setRerunning] = useState(false);
  const [view, setView] = useState<View>(() => (window.matchMedia?.('(max-width: 640px)').matches ? 'list' : 'lanes'));
  const [scale, setScale] = useState<Scale>('step');
  const notice = (loc.state as { notice?: string } | null)?.notice ?? null;
  const selected = sp.get('step') != null ? Number(sp.get('step')) : null;
  const select = useCallback((seq: number | null) => {
    setSp((p) => { const n = new URLSearchParams(p); if (seq == null) n.delete('step'); else n.set('step', String(seq)); return n; }, { replace: true, state: loc.state });
  }, [setSp, loc.state]);

  // A response only lands if it is still for the branch on screen.
  const current = useRef(branchId);
  current.current = branchId;
  const load = useCallback(() => {
    const id = branchId;
    arena.branch(id).then((d) => { if (current.current === id) { setDetail(d); setError(null); } })
      .catch((e) => { if (current.current === id) setError(e); });
    arena.runs().then((r) => {
      if (current.current !== id) return;
      setSiblings(r.find((x) => x.id === run)?.branches ?? []);
    }).catch(() => {});
  }, [branchId, run]);

  useEffect(() => { setDetail(null); setRerunning(false); load(); }, [load]);

  useEffect(() => {
    const task = detail?.events.find((e) => e.kind === 'run_start')?.payload as { task_id?: string } | undefined;
    if (!task?.task_id) return;
    arena.tasks().then((ts) => setReason(ts.find((t) => t.id === task.task_id)?.reason ?? null)).catch(() => {});
  }, [detail?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const running = detail?.status === 'running';
  useEffect(() => {
    if (!running) return;
    return follow(
      branchId,
      (e: Event) => setDetail((d) => (d && d.id === branchId && !d.events.some((x) => x.seq === e.seq) ? { ...d, events: [...d.events, e] } : d)),
      () => load(),
    );
  }, [running, branchId, load]);

  // While running, the clock keeps moving.
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [running]);

  const lay = useMemo(() => (detail ? layout(detail.events, detail.team, detail.start_seq) : null), [detail]);
  const order = useMemo(() => (lay ? inOrder(lay) : []), [lay]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable || e.metaKey || e.ctrlKey || e.altKey) return;
      const dir = e.key === 'ArrowRight' || e.key === 'j' ? 1 : e.key === 'ArrowLeft' || e.key === 'k' ? -1 : 0;
      if (!dir || !order.length) return;
      e.preventDefault();
      const i = selected == null ? (dir > 0 ? -1 : order.length) : order.findIndex((s) => s.seq === selected);
      select(order[Math.min(order.length - 1, Math.max(0, i + dir))].seq);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [order, selected, select]);

  if (!detail || !lay) {
    return (
      <main className="page page-list tst-page">
        <Crumbs task={null} />
        {error ? <ErrorNote error={error} /> : <p className="muted">Loading the run…</p>}
      </main>
    );
  }

  const sm = summarize(detail, detail.events, now);
  const taskId = (detail.events.find((e) => e.kind === 'run_start')?.payload as { task_id?: string } | undefined)?.task_id ?? null;
  const event = detail.events.find((e) => e.seq === selected) ?? null;
  const step = order.find((s) => s.seq === selected) ?? null;
  const names = branchNames(siblings.length ? siblings : [detail]);
  const others = siblings.filter((b) => b.id !== detail.id && b.status !== 'running');
  const canRerun = detail.status === 'running' ? 'Wait for this branch to finish first.'
    : !event || event.kind === 'check' || event.kind === 'run_start' ? 'The final check can’t be rerun; pick a step an agent took.'
    : null;
  const world = worldLine(detail.events);
  const parent = detail.parent_id ? siblings.find((b) => b.id === detail.parent_id) : null;

  return (
    <main className="page page-list tst-page tst-run-page">
      <Crumbs task={taskId} />
      <header className="tst-run-header">
        <div className="tst-run-heading">
          <h1 className="page-title">Task {taskId ?? '?'} <StatusPill status={detail.status} /> <ApproximateMark of={detail} /></h1>
          {reason && <p className="muted tst-lede" title={reason}>{reason}</p>}
          {world && <p className="muted tst-small tst-world" title={world}>{world}</p>}
        </div>
        <div className="tst-run-actions">
          {others.length > 0 && (
            <label className="tst-inline">
              <span className="muted tst-small">Compare with</span>
              <select value="" onChange={(e) => e.target.value && nav(comparePath(detail.id, e.target.value))}>
                <option value="">choose…</option>
                {others.map((b) => <option key={b.id} value={b.id}>{names.get(b.id)}</option>)}
              </select>
            </label>
          )}
          <Btn variant="primary" icon="refresh" disabled={canRerun != null} onClick={() => setRerunning(true)}
            tip={canRerun ?? 'Change a prompt, model or agent and rerun from the selected step'}>Change &amp; rerun</Btn>
        </div>
      </header>

      {siblings.length > 1 && (
        <nav className="tst-branchbar" aria-label="Branches of this run">
          {branchTree(siblings).map(({ branch: b, depth }) => (
            <Link key={b.id} to={runPath(run, b.id)} className={`tst-branch-tab${b.id === detail.id ? ' on' : ''}`} aria-current={b.id === detail.id ? 'page' : undefined}>
              {depth > 0 && <span className="muted" aria-hidden="true">↳</span>}
              <span>{names.get(b.id)}</span>
              <StatusPill status={b.status} short />
              <ApproximateMark of={b} />
            </Link>
          ))}
        </nav>
      )}

      {notice && <div className="tst-note" role="status">{notice}</div>}
      {detail.fork_note && (
        <div className="tst-note">
          <Icon name="route" size={15} />
          <span>{parent ? <>Rerun of <Link to={runPath(run, parent.id)}>{names.get(parent.id)}</Link>: </> : null}{detail.fork_note}</span>
        </div>
      )}
      {detail.error && <div className="tst-note tst-note-error"><span>This branch could not finish.</span><span className="tst-evidence mono" data-raw-evidence>{detail.error}</span></div>}
      <ErrorNote error={error} />

      <section className="tst-stats" aria-label="Totals">
        <Stat label={sm.live ? 'Time so far' : 'Time'} value={sm.seconds != null ? `${sm.seconds.toFixed(1)} s` : 'unknown'} note={detail.start_seq != null ? `since step ${detail.start_seq}` : undefined} />
        <Stat label="Tokens" value={sm.tokens ? sm.tokens.toLocaleString() : sm.live ? '…' : '0'} note="reported by each agent" />
        <Stat label="Cost" value={sm.cost_usd != null ? dollars(sm.cost_usd) : 'tokens only'} note={sm.cost_usd != null ? costSource(sm.cost_source) : 'no dollar figure from every agent'} />
        <Stat label="Rows changed" value={sm.rows_written} note="by this branch" />
        <Stat label="Steps" value={sm.steps} note={detail.start_seq != null ? 'run by this branch' : undefined} />
      </section>

      <div className="tst-run-body">
        <section className="tst-card tst-timeline" aria-label="Steps">
          <header className="tst-card-head">
            <h2 className="tst-h2">Steps</h2>
            <span className="tst-spacer" />
            {view === 'lanes' && <Segmented label="Space the steps" value={scale} onChange={setScale} options={[{ value: 'step', label: 'By step', tip: 'One slot per step, in order' }, { value: 'time', label: 'By time', tip: 'Width is the time each step took' }]} />}
            <Segmented label="Show steps as" value={view} onChange={setView} options={[{ value: 'lanes', label: 'Lanes', icon: 'activity' }, { value: 'list', label: 'List', icon: 'list' }]} />
          </header>
          {view === 'lanes'
            ? <><div className="tst-lanes-wrap"><Swimlanes layout={scale === 'step' ? bySteps(lay) : lay} team={detail.team} scale={scale} selected={selected} onSelect={select} /></div><Legend /></>
            : <StepList layout={lay} team={detail.team} selected={selected} onSelect={select} />}
        </section>
        <aside className="tst-side" aria-label={rerunning ? 'Change and rerun' : 'Selected step'}>
          {rerunning && selected != null && canRerun == null ? (
            <RerunPanel
              key={`${detail.id}:${selected}`}
              branch={detail}
              seq={selected}
              openAvailable={!!health?.open_available}
              onClose={() => setRerunning(false)}
              onDone={(r) => nav(runPath(run, r.branch_id), {
                state: { notice: r.moved_from != null ? `Started from step ${r.start_seq} instead of ${r.moved_from}: ${r.reason ?? ''}` : `Rerunning from step ${r.start_seq}.` },
              })}
            />
          ) : (
            <div className="tst-card"><StepInspector branch={detail} event={event} step={step} canRerun={canRerun} onRerun={() => setRerunning(true)} /></div>
          )}
        </aside>
      </div>
    </main>
  );
}

function Crumbs({ task }: { task: string | null }) {
  return (
    <nav className="tst-crumbs" aria-label="Breadcrumb">
      <Link to="/tests">Tests</Link>
      <span aria-hidden="true">/</span>
      <span>{task ? `Task ${task}` : 'Run'}</span>
    </nav>
  );
}
