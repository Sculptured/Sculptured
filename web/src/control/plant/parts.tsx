import type { Basis, Outcome, Permit, PlantAction, SystemUnit } from './model';
import { effectCode, isAbnormal, kindCode, kindWords, OUTCOME_WORDS, systemLine } from './model';

/** Small shared pieces of the plant panes: a system's badge, an outcome, a permit's state. */

/** The system's kind as a short code ("ERP"); its words are the title. */
export function KindBadge({ s }: { s: SystemUnit }) {
  return <span className={`pl-kind${s.kind === 'unknown' ? ' is-unknown' : ''}`} title={kindWords(s.kind)}>{kindCode(s.kind)}</span>;
}

/** Name, then what it is: product, what it stands in for, simulated or real. */
export function SystemName({ s }: { s: SystemUnit }) {
  const line = systemLine(s);
  return (
    <span className="pl-sys-name">
      <strong>{s.name}</strong>
      {line && <span className="pl-sys-line">{line}</span>}
    </span>
  );
}

export function OutcomeMark({ o }: { o: Outcome }) {
  const w = OUTCOME_WORDS[o];
  return <span className={`pl-outcome${isAbnormal(o) ? ' is-abnormal' : ''}`}><span aria-hidden="true">{w.glyph}</span> {w.word}</span>;
}

const BASIS_WORDS: Record<Basis, string> = {
  declared: 'as the system declares',
  observed: 'observed: it wrote rows',
  sculptured: "Sculptured's own tool knowledge",
  unknown: 'not declared',
};
export const basisWords = (b: Basis) => BASIS_WORDS[b];

/** "W" or "R" (or "?"), heavier for a write. */
export function EffectMark({ a }: { a: Pick<PlantAction, 'effect' | 'effectBasis'> }) {
  const word = a.effect === 'unknown' ? 'effect unknown' : a.effect;
  return <span className={`pl-effect is-${a.effect}`} title={`${word} (${basisWords(a.effectBasis)})`}>{effectCode(a.effect)}<span className="sr-only"> {word}</span></span>;
}

const PERMIT: Record<Permit['state'], { glyph: string; word: string }> = {
  waiting: { glyph: '◇', word: 'permit waiting' },
  granted: { glyph: '✓', word: 'permit granted' },
  refused: { glyph: '⊘', word: 'permit refused' },
};

export function PermitState({ p }: { p: Permit }) {
  const w = PERMIT[p.state];
  return (
    <span className={`pl-permit is-${p.state}`}>
      <span aria-hidden="true">{w.glyph}</span> {w.word}{p.by ? ` by ${p.by}` : ''}
    </span>
  );
}
