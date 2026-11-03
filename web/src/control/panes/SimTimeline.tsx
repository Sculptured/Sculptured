import { useState, type KeyboardEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { runPath } from '../../arena/paths';
import { Segmented } from '../../shell/ui';
import { bySteps, type Step } from '../../tests/model';
import { RerunPanel } from '../../tests/RerunPanel';
import { WAIT_REASON } from '../../tests/RestoreChoice';
import { savedParts } from '../../tests/restore';
import { StepInspector } from '../../tests/StepInspector';
import { StepList } from '../../tests/Swimlanes';
import type { BranchDetail, Change, RestoreMode } from '../../tests/types';
import type { PaneProps } from '../../workspace/registry';
import { alarmIdOf, useControl } from '../context';
import type { ControlSource, SimulatedRun, SimulatedTimeline } from '../source';
import { stepFrom, useCursor } from './cursor';
import { StepLanes, StepLegend, type Scale } from './StepLanes';
import type { TimelineSettings } from './Timeline';

/**
 * The replay: a simulated run step by step. Its lanes (or the list), the step at the cursor in full
 * beside them, and "Fork from here": the fork panel with each agent's instructions, the Exact or
 * Approximate restore and the per-part plan. ←/→ and j/k step through the steps, Home and End go
 * to the first and last; a fork that starts opens the new branch's workspace.
 */

export const CHECK_NOT_FORKABLE = 'The final check can’t be forked from; pick a step an agent took.';

/** Why "Fork from here" can't start: the branch still runs, or the step is the final check. */
export function forkWhy(b: BranchDetail, step: Step | undefined): string | null {
  if (b.status === 'running') return WAIT_REASON;
  return !step || step.kind === 'check' ? CHECK_NOT_FORKABLE : null;
}

const typing = (t: EventTarget) => {
  const el = t as HTMLElement;
  return /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName) || el.isContentEditable || !!el.closest?.('.tst-rerun');
};

/** ←/→, j/k, Home and End, unless the operator is typing or in the fork panel. */
function stepKeys(steps: Step[], cursor: number, move: (seq: number) => void) {
  return (e: KeyboardEvent) => {
    if (typing(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
    const by = e.key === 'ArrowRight' || e.key === 'j' ? 1 : e.key === 'ArrowLeft' || e.key === 'k' ? -1 : e.key === 'Home' ? 'first' : e.key === 'End' ? 'last' : 0;
    if (!by) return;
    const next = stepFrom(steps, cursor, by);
    if (next === undefined) return;
    e.preventDefault();
    move(next);
  };
}

/** Saves by step, with the parts each saved. */
function savesOf(b: BranchDetail, saves: number[]): Map<number, string[]> {
  return new Map(saves.map((seq) => [seq, savedParts((b.events.find((e) => e.seq === seq)?.payload ?? {}) as Record<string, never>)]));
}

/** Steps whose feed entry raised an alarm that is up now. */
function alarmSteps(source: ControlSource, raised: Set<string>): Set<number> {
  const out = new Set<number>();
  for (const i of source.feed) {
    const id = alarmIdOf(i);
    const seq = (i.raw as { seq?: unknown } | null)?.seq;
    if (id && raised.has(id) && typeof seq === 'number') out.add(seq);
  }
  return out;
}

type Bar = { tl: SimulatedTimeline; step?: Step; why: string | null; forking: boolean; s: TimelineSettings; set(s: TimelineSettings): void; move(seq: number): void; fork(): void };

function Toolbar({ tl, step, why, forking, s, set, move, fork }: Bar) {
  const i = tl.steps.findIndex((x) => x.seq === tl.cursor);
  const go = (by: number) => { const n = stepFrom(tl.steps, tl.cursor, by); if (n !== undefined) move(n); };
  return (
    <div className="ctl-replay-bar" role="toolbar" aria-label="Replay controls">
      <button type="button" className="btn btn-sm btn-secondary" aria-label="Previous step" aria-keyshortcuts="ArrowLeft K" disabled={i <= 0} onClick={() => go(-1)}>◀</button>
      <button type="button" className="btn btn-sm btn-secondary" aria-label="Next step" aria-keyshortcuts="ArrowRight J" disabled={i < 0 || i >= tl.steps.length - 1} onClick={() => go(1)}>▶</button>
      <span className="ctl-replay-at" aria-live="polite">Step <b>{tl.cursor}</b> <span className="ctl-replay-of">{i + 1}/{tl.steps.length}</span>{step && <span className="ctl-replay-label"> {step.label}</span>}</span>
      <span className="ctl-replay-keys" aria-hidden="true">
        <span className="is-idle">Click a step, then ← → or j k</span>
        <span className="is-on"><kbd>←</kbd><kbd>→</kbd> <kbd>j</kbd><kbd>k</kbd> step · <kbd>Home</kbd><kbd>End</kbd></span>
      </span>
      <span className="ctl-replay-gap" />
      {s.view !== 'list' && <Segmented label="Space the steps" value={s.scale ?? 'step'} onChange={(v: Scale) => set({ ...s, scale: v })}
        options={[{ value: 'step', label: 'By step', tip: 'One slot per step, in order' }, { value: 'time', label: 'By time', tip: 'Where each step was recorded in time' }]} />}
      <Segmented label="Show steps as" value={s.view ?? 'lanes'} onChange={(v: 'lanes' | 'list') => set({ ...s, view: v })} options={[{ value: 'lanes', label: 'Lanes' }, { value: 'list', label: 'List' }]} />
      <span className="ctl-act">
        <button type="button" className="btn btn-sm btn-primary primary" disabled={!!why || forking} aria-expanded={forking} onClick={fork}><span aria-hidden="true">⑂</span> Fork from here</button>
        {why && <span className="ctl-why">{why}</span>}
      </span>
    </div>
  );
}

const AROUND = 3;

/** The steps either side of the cursor as a list: what happened just before and after it. */
function Around({ tl, b, move }: { tl: SimulatedTimeline; b: BranchDetail; move(seq: number): void }) {
  const i = Math.max(0, tl.steps.findIndex((x) => x.seq === tl.cursor));
  const near = new Set(tl.steps.slice(Math.max(0, i - AROUND), i + AROUND + 1).map((x) => x.seq));
  const lay = { ...tl.layout, lanes: tl.layout.lanes.map((l) => ({ ...l, steps: l.steps.filter((x) => near.has(x.seq)) })) };
  return (
    <section className="ctl-around" aria-label={`Steps around step ${tl.cursor}`}>
      <h3 className="ctl-sub">Around step {tl.cursor}</h3>
      <StepList layout={lay} team={b.team} selected={tl.cursor} onSelect={move} />
    </section>
  );
}

type Side = { a: SimulatedRun; b: BranchDetail; tl: SimulatedTimeline; step?: Step; forking: boolean; close(): void };

function SideColumn({ a, b, tl, step, forking, close }: Side) {
  const nav = useNavigate();
  const rows = useControl().source.world!.read;
  const start = async (changes: Record<string, Change>, mode: RestoreMode) => {
    const r = await tl.fork(tl.cursor, changes, mode);
    nav(runPath(r.run, r.branch), { state: { notice: r.notice } });
  };
  if (forking) {
    return <RerunPanel key={b.id} agents={b.team.agents} seq={tl.cursor} running={b.status === 'running'} openAvailable={a.openAvailable} plan={tl.plan} start={start} onClose={close} />;
  }
  return <div className="tst-card"><StepInspector branch={b} event={b.events.find((e) => e.seq === tl.cursor) ?? null} step={step ?? null} rows={rows} /></div>;
}

/** The lanes (with the legend and the steps around the cursor), or every step as a list. */
function Steps({ s, tl, b, move }: { s: TimelineSettings; tl: SimulatedTimeline; b: BranchDetail; move(seq: number): void }) {
  const { source, alarms } = useControl();
  if (s.view === 'list') return <StepList layout={tl.layout} team={b.team} selected={tl.cursor} onSelect={move} />;
  const hidden = new Set([...(s.hiddenLanes ?? []), ...(s.alarmsLane === false ? ['alarms'] : [])]);
  const scale = s.scale ?? 'step';
  return (
    <>
      <StepLanes layout={scale === 'step' ? bySteps(tl.layout) : tl.layout} scale={scale} agents={source.agents} cursor={tl.cursor}
        saves={savesOf(b, tl.saves)} alarms={alarmSteps(source, new Set(alarms.list.map((x) => x.id)))} hidden={hidden} onPick={move} />
      <StepLegend />
      <Around tl={tl} b={b} move={move} />
    </>
  );
}

export function SimTimeline({ settings, setSettings }: PaneProps) {
  const s = settings as TimelineSettings;
  const a = useControl().source.arena!;
  const tl = useControl().source.timeline!;
  const b = a.branch!;
  const move = useCursor(tl, b);
  const [forking, setForking] = useState(false);
  const step = tl.steps.find((x) => x.seq === tl.cursor);
  const why = forkWhy(b, step);
  // The fork panel closes once a fork can't start here (the cursor moved to the check, or the branch runs).
  if (forking && why) setForking(false);
  const open = forking && !why;
  return (
    <div className="ctl-replay" onKeyDown={stepKeys(tl.steps, tl.cursor, move)}>
      <Toolbar tl={tl} step={step} why={why} forking={open} s={s} set={(n) => setSettings(n as Record<string, unknown>)} move={move} fork={() => setForking(true)} />
      <div className="ctl-replay-body">
        <div className="ctl-replay-main"><Steps s={s} tl={tl} b={b} move={move} /></div>
        <aside className="ctl-replay-side" aria-label={open ? 'Fork from here' : 'Selected step'}>
          <SideColumn a={a} b={b} tl={tl} step={step} forking={open} close={() => setForking(false)} />
        </aside>
      </div>
    </div>
  );
}
