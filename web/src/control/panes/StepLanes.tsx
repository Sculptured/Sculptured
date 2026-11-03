import type { CSSProperties, ReactNode } from 'react';
import { CHECK_LANE, inOrder, ticks, type Layout, type Step } from '../../tests/model';
import { KIND, kindWord } from '../../tests/Swimlanes';
import type { AgentLine } from '../source';

/**
 * The replay's lanes: one per agent (then the check), its steps as marks in order or on time, a
 * lane of saves and a lane of alarms, the cursor across every lane and, on a fork, where it
 * started. Each step is a button (the cursor's is the one in the tab order; the pane's arrow keys
 * move it). Every mark is a glyph with its words in its label, never colour alone.
 */

export type Scale = 'step' | 'time';

type LanesProps = {
  layout: Layout;
  scale: Scale;
  agents: AgentLine[];
  cursor: number;
  /** Saves by step: the parts saved after it. */
  saves: Map<number, string[]>;
  /** Steps whose entry raised an alarm. */
  alarms: Set<number>;
  hidden: Set<string>;
  onPick(seq: number): void;
};

const pct = (v: number, span: number) => `${(100 * v) / span}%`;
/** Where a step's mark sits: the middle of its slot (by step) or the moment it was recorded (by time). */
const spot = (s: Step, scale: Scale) => (scale === 'step' ? s.from + 0.5 : s.to);

function stepWords(s: Step, saved: string[] | undefined): string {
  const k = kindWord(s);
  return `Step ${s.seq}: ${s.label}${k ? ` · ${k}` : ''}${s.failed ? ' · failed' : ''}${s.inherited ? ' · reused from the original' : ''}${saved ? ` · saved ${saved.join(', ')}` : ''}`;
}

function Mark({ s, p, on, saved }: { s: Step; p: LanesProps; on: boolean; saved?: string[] }) {
  const span = p.layout.span;
  const point = s.kind === 'start' || s.kind === 'end' || p.scale === 'time';
  const style: CSSProperties = point ? { left: pct(spot(s, p.scale), span) } : { left: pct(s.from, span), width: `calc(${pct(s.to - s.from, span)} - 2px)` };
  const cls = `ctl-step k-${s.kind}${point ? ' is-point' : ''}${s.inherited ? ' is-inherited' : ''}${s.failed ? ' is-failed' : ''}${on ? ' is-cursor' : ''}`;
  return (
    <button type="button" className={cls} style={style} tabIndex={on ? 0 : -1} aria-pressed={on} aria-label={stepWords(s, saved)} title={stepWords(s, saved)} onClick={() => p.onPick(s.seq)}>
      <span className="ctl-step-glyph" aria-hidden="true">{s.failed ? '✕' : KIND[s.kind].glyph}</span>
      {!point && <span className="ctl-step-label">{s.label}</span>}
      {saved && <span className="ctl-step-saved" aria-hidden="true">◆</span>}
    </button>
  );
}

/** The cursor and the fork's start, drawn in every track so they line up across lanes. */
function Rules({ p, cursorAt }: { p: LanesProps; cursorAt: number | null }) {
  const { forkAt, span } = p.layout;
  return (
    <>
      {forkAt != null && <span className="ctl-forkline" style={{ left: pct(forkAt, span) }} aria-hidden="true" />}
      {cursorAt != null && <span className="ctl-cursor" style={{ left: pct(cursorAt, span) }} aria-hidden="true" />}
    </>
  );
}

function Lane({ name, label, cls, children, p, cursorAt }: { name: string; label: ReactNode; cls?: string; children: ReactNode; p: LanesProps; cursorAt: number | null }) {
  return (
    <li className={`ctl-lane ctl-steps-lane${cls ? ` ${cls}` : ''}`}>
      <span className="ctl-lane-name">{label}</span>
      <div className="ctl-steps-track" role="group" aria-label={name}>
        <Rules p={p} cursorAt={cursorAt} />
        {children}
      </div>
    </li>
  );
}

function AgentLabel({ a }: { a?: AgentLine }) {
  if (!a) return <><span className="ctl-glyph" aria-hidden="true">✓</span><span className="ctl-lane-who">Check</span><span className="ctl-lane-word">world vs. expected</span></>;
  return (
    <>
      <span className={`ctl-glyph tone-${a.status.tone}`} aria-hidden="true">{a.status.glyph}</span>
      <span className="ctl-lane-who">{a.name}</span>
      <span className="ctl-lane-word">{a.status.word}</span>
    </>
  );
}

/** A lane of marks at given steps (saves, alarms), each a button that moves the cursor there. */
function MarkLane({ title, glyph, cls, marks, p, cursorAt, order }: { title: string; glyph: string; cls: string; marks: { seq: number; words: string }[]; p: LanesProps; cursorAt: number | null; order: Step[] }) {
  const at = new Map(order.map((s) => [s.seq, s]));
  return (
    <Lane name={title} p={p} cursorAt={cursorAt} cls={cls}
      label={<><span className="ctl-glyph" aria-hidden="true">{glyph}</span><span className="ctl-lane-who">{title}</span><span className="ctl-lane-word">{marks.length}</span></>}>
      {marks.map((m) => {
        const s = at.get(m.seq);
        if (!s) return null;
        return <button key={m.seq} type="button" tabIndex={-1} className={`ctl-lane-mark${m.seq === p.cursor ? ' is-cursor' : ''}`} style={{ left: pct(spot(s, p.scale), p.layout.span) }}
          aria-label={m.words} title={m.words} onClick={() => p.onPick(m.seq)}><span aria-hidden="true">{glyph}</span></button>;
      })}
    </Lane>
  );
}

function Axis({ p, order }: { p: LanesProps; order: Step[] }) {
  const span = p.layout.span;
  const every = Math.max(1, Math.ceil(order.length / 12));
  const marks: [number, string][] = p.scale === 'time' ? ticks(span) : order.filter((_, i) => i % every === 0).map((s) => [s.from + 0.5, String(s.seq)]);
  return (
    <div className="ctl-axis ctl-steps-axis" aria-hidden="true">
      <span className="ctl-lane-name">▼ Step {p.cursor}</span>
      <span className="ctl-steps-ticks">{marks.map(([v, l]) => <span key={`${v}`} className="ctl-steps-tick" style={{ left: pct(v, span) }}>{l}</span>)}</span>
    </div>
  );
}

export function StepLanes(p: LanesProps) {
  const order = inOrder(p.layout);
  const current = order.find((s) => s.seq === p.cursor);
  const cursorAt = current ? spot(current, p.scale) : null;
  const byId = new Map(p.agents.map((a) => [a.id, a]));
  const lanes = p.layout.lanes.filter((l) => !p.hidden.has(l.agent));
  const saves = [...p.saves].map(([seq, parts]) => ({ seq, words: `Saved after step ${seq}: ${parts.join(', ')}` }));
  const alarms = [...p.alarms].map((seq) => ({ seq, words: `Alarm at step ${seq}` }));
  return (
    <div className="ctl-steps" style={{ ['--steps' as string]: order.length }}>
      <Axis p={p} order={order} />
      <ul className="ctl-lanes ctl-steps-lanes" aria-label="Steps by agent">
        {lanes.map((l) => (
          <Lane key={l.agent} name={`${l.agent === CHECK_LANE ? 'Check' : byId.get(l.agent)?.name ?? l.agent} steps`} p={p} cursorAt={cursorAt}
            cls={current && l.steps.includes(current) ? 'is-picked' : undefined} label={<AgentLabel a={byId.get(l.agent)} />}>
            {l.steps.map((s) => <Mark key={s.seq} s={s} p={p} on={s.seq === p.cursor} saved={p.saves.get(s.seq)} />)}
          </Lane>
        ))}
        <MarkLane title="Saves" glyph="◆" cls="ctl-saves-lane" marks={saves} p={p} cursorAt={cursorAt} order={order} />
        {!p.hidden.has('alarms') && <MarkLane title="Alarms" glyph="▲" cls="ctl-alarms-lane" marks={alarms} p={p} cursorAt={cursorAt} order={order} />}
      </ul>
    </div>
  );
}

/** What each mark means, in words. */
export function StepLegend() {
  return (
    <div className="ctl-steps-legend" aria-label="Step kinds">
      {(['start', 'read', 'write', 'model', 'handoff', 'end', 'check'] as const).map((k) => (
        <span key={k} className={`ctl-legend-item k-${k}`}><span className="ctl-step-glyph" aria-hidden="true">{KIND[k].glyph}</span>{KIND[k].word}</span>
      ))}
      <span className="ctl-legend-item"><span aria-hidden="true">◆</span>Saved after the step</span>
      <span className="ctl-legend-item"><span className="ctl-legend-hatch" aria-hidden="true" />Reused from the original</span>
      <span className="ctl-legend-item"><span className="ctl-legend-fork" aria-hidden="true" />Fork starts</span>
    </div>
  );
}
