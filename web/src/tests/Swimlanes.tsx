import { CHECK_LANE, inOrder, ticks, type Layout, type Step, type StepKind } from './model';
import { KindMark } from './shared';
import type { Team } from './types';

/** Each kind of step has its own shape; colour only repeats it. */
export const KIND: Record<StepKind, { glyph: string; word: string }> = {
  start: { glyph: '▸', word: 'Agent starts' },
  model: { glyph: '◇', word: 'Model call' },
  read: { glyph: '○', word: 'Reads the database' },
  write: { glyph: '✎', word: 'Writes the database' },
  handoff: { glyph: '⇢', word: 'Hands work on' },
  end: { glyph: '■', word: 'Agent finishes' },
  check: { glyph: '✓', word: 'Final check' },
};

/** Blocks sit on time (width = time spent) or on equal slots in step order (labels always fit). */
export type Scale = 'time' | 'step';

const pct = (v: number, span: number) => `${(100 * v) / span}%`;
const secs = (ms: number) => `${(ms / 1000).toFixed(1)} s`;

function Block({ s, span, scale, selected, onSelect }: { s: Step; span: number; scale: Scale; selected: boolean; onSelect: (seq: number) => void }) {
  const k = KIND[s.kind];
  const point = s.kind === 'start' || s.kind === 'end' || (scale === 'time' && s.to - s.from < span / 400);
  const title = `Step ${s.seq}: ${s.label} · ${k.word}${s.inherited ? ' · reused from the original' : ''}${s.failed ? ' · failed' : ''}`;
  // On the step scale a point sits mid-slot; blocks leave a small gap to the next slot.
  const left = point && scale === 'step' ? pct(s.from + 0.5, span) : pct(s.from, span);
  return (
    <button
      type="button"
      className={`tst-block k-${s.kind}${point ? ' point' : ''}${s.inherited ? ' inherited' : ''}${s.failed ? ' failed' : ''}${selected ? ' selected' : ''}`}
      style={{ left, width: point ? undefined : `calc(${pct(s.to - s.from, span)} - 3px)` }}
      aria-pressed={selected}
      aria-label={title}
      data-tip={title}
      onClick={() => onSelect(s.seq)}
    >
      <span className="tst-glyph" aria-hidden="true">{s.failed ? '✕' : k.glyph}</span>
      {!point && <span className="tst-block-label">{s.label}</span>}
      {s.snapshot && <span className="tst-snap" aria-hidden="true" />}
    </button>
  );
}

/** One lane per agent across time, the check last. Click a block to inspect that step. */
export function Swimlanes({ layout, team, scale, selected, onSelect }: {
  layout: Layout;
  team: Team;
  scale: Scale;
  selected: number | null;
  onSelect: (seq: number) => void;
}) {
  const { lanes, span, forkAt } = layout;
  const marks = scale === 'time' ? ticks(span) : stepTicks(layout);
  const agent = (id: string) => team.agents.find((a) => a.id === id);
  // A handoff connects its lane to the next one, at the moment it was recorded.
  const links = lanes.flatMap((l, i) =>
    i + 1 < lanes.length ? l.steps.filter((s) => s.kind === 'handoff').map((s) => ({ at: s.to, row: i, seq: s.seq })) : []);
  return (
    <div className="tst-lanes" style={{ ['--lanes' as string]: lanes.length, ['--min-track' as string]: scale === 'step' ? `${span * 30}px` : '420px' }}>
      <div className="tst-axis-name" style={{ gridRow: 1, gridColumn: 1 }} />
      <div className="tst-axis" aria-hidden="true" style={{ gridRow: 1, gridColumn: 2 }}>
        {marks.map(([v, label]) => <span key={v} className="tst-tick" style={{ left: pct(v, span) }}>{label}</span>)}
      </div>
      {lanes.map((l, i) => {
        const a = agent(l.agent);
        return [
          <div className="tst-lane-name" key={`${l.agent}-n`} style={{ gridRow: i + 2, gridColumn: 1 }}>
            <strong>{l.agent === CHECK_LANE ? 'Check' : a?.name ?? l.agent}</strong>
            {a ? <KindMark kind={a.kind} /> : <span className="muted tst-small">database vs. expected</span>}
          </div>,
          <div className="tst-track" key={`${l.agent}-t`} style={{ gridRow: i + 2, gridColumn: 2 }} role="group" aria-label={`${a?.name ?? 'Check'} steps`}>
            {marks.map(([v]) => <span key={v} className="tst-grid" style={{ left: pct(v, span) }} />)}
            {l.steps.map((s) => <Block key={s.seq} s={s} span={span} scale={scale} selected={selected === s.seq} onSelect={onSelect} />)}
          </div>,
        ];
      })}
      <div className="tst-overlay" aria-hidden="true" style={{ gridRow: `1 / ${lanes.length + 2}`, gridColumn: 2 }}>
        {forkAt != null && (
          <span className="tst-forkline" style={{ left: pct(forkAt, span) }}><span>Rerun starts here</span></span>
        )}
        {links.map((h) => (
          <span key={h.seq} className="tst-handoff" style={{ left: pct(h.at, span), top: `calc(var(--axis-h) + (${h.row} + 0.5) * var(--lane-h))` }} />
        ))}
      </div>
    </div>
  );
}

/** On the step scale: the step number at the start of every few slots. */
function stepTicks(l: Layout): [number, string][] {
  const every = Math.max(1, Math.ceil(l.span / 12));
  return inOrder(l).filter((_, i) => i % every === 0).map((s) => [s.from, `step ${s.seq}`]);
}

/** The same steps as a list: the text view of the lanes, and the one used on narrow screens. */
export function StepList({ layout, team, selected, onSelect }: {
  layout: Layout;
  team: Team;
  selected: number | null;
  onSelect: (seq: number) => void;
}) {
  const rows = layout.lanes.flatMap((l) => l.steps.map((s) => ({ s, lane: l.agent }))).sort((a, b) => a.s.seq - b.s.seq);
  const name = (id: string) => (id === CHECK_LANE ? 'Check' : team.agents.find((a) => a.id === id)?.name ?? id);
  return (
    <table className="data-table tst-steplist">
      <thead><tr><th>Step</th><th>At</th><th>Agent</th><th>What happened</th></tr></thead>
      <tbody>
        {rows.map(({ s, lane }) => (
          <tr key={s.seq} className={`${selected === s.seq ? 'selected' : ''}${s.inherited ? ' inherited' : ''}`}>
            <td className="mono">{s.seq}</td>
            <td className="mono muted">{secs(s.to)}</td>
            <td>{name(lane)}</td>
            <td>
              <button type="button" className="tst-linkbtn" onClick={() => onSelect(s.seq)} aria-pressed={selected === s.seq}>
                <span aria-hidden="true">{s.failed ? '✕' : KIND[s.kind].glyph}</span> {s.label}
              </button>
              <span className="muted tst-small"> {KIND[s.kind].word}{s.inherited ? ' · reused from the original' : ''}</span>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function Legend() {
  return (
    <div className="tst-legend" aria-label="Step kinds">
      {(['read', 'write', 'model', 'handoff', 'check'] as StepKind[]).map((k) => (
        <span key={k} className={`tst-legend-item k-${k}`}><span className="tst-glyph" aria-hidden="true">{KIND[k].glyph}</span>{KIND[k].word}</span>
      ))}
      <span className="tst-legend-item"><span className="tst-snap static" aria-hidden="true" />Saved after the step</span>
      <span className="tst-legend-item inherited"><span className="tst-hatch" aria-hidden="true" />Reused from the original</span>
    </div>
  );
}
