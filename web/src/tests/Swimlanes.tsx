import { CHECK_LANE, type Layout, type StepKind } from './model';
import type { Team } from './types';

/** Each kind of step has its own shape; colour only repeats it. */
export const KIND: Record<StepKind, { glyph: string; word: string }> = {
  start: { glyph: '▸', word: 'Agent starts' },
  model: { glyph: '◇', word: 'Model call' },
  read: { glyph: '○', word: 'Reads the world' },
  write: { glyph: '✎', word: 'Writes to the world' },
  handoff: { glyph: '⇢', word: 'Hands work on' },
  end: { glyph: '■', word: 'Agent finishes' },
  check: { glyph: '✓', word: 'Final check' },
};

const secs = (ms: number) => `${(ms / 1000).toFixed(1)} s`;

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
