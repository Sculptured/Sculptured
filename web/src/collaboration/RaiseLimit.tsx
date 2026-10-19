import { useState } from 'react';
import { errorText } from '../errors';
import { compactNumber } from '../format';
import { Btn } from '../shell/ui';

export type LimitUnit = 'tokens' | 'minutes' | 'percent' | 'turns';

/** A limit in words: `2.0M`, `60 min`, `30%`, `4 turns`. */
export function limitWords(unit: LimitUnit, v: number): string {
  if (unit === 'percent') return `${+v.toFixed(1)}%`;
  if (unit === 'minutes') return `${Math.floor(v)} min`;
  if (unit === 'turns') return `${v} turns`;
  return compactNumber(Math.round(v));
}

/** Half again as much, rounded the way the limit is counted (a plan share never past 100%). */
export function suggestedRaise(unit: LimitUnit, hard: number): number {
  if (unit === 'percent') return Math.min(100, Math.round(hard * 15) / 10);
  return Math.ceil(hard * 1.5);
}

/** "Raise …": a higher limit for this run only (the saved budget stays as planned). `continues`: the
 * run or agent stopped or is wrapping up at this limit, and carries on once it is raised. */
export function RaiseLimit({ label, unit, hard, continues, send }: { label: string; unit: LimitUnit; hard: number; continues: boolean; send: (hard: number) => Promise<unknown> }) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState(String(suggestedRaise(unit, hard)));
  const [state, setState] = useState<{ busy: boolean; done: boolean; problem: string }>({ busy: false, done: false, problem: '' });
  if (state.done) return <span className="muted" role="status">Raised for this run. The saved budget is unchanged.</span>;
  if (!open) return <Btn size="sm" aria-label={`Raise ${label}`} onClick={() => setOpen(true)}>Raise</Btn>;
  const submit = async () => {
    const n = Number(value);
    if (!Number.isFinite(n) || n <= hard) return setState({ ...state, problem: `Choose more than ${limitWords(unit, hard)}.` });
    if (unit === 'percent' && n > 100) return setState({ ...state, problem: 'Choose at most 100%.' });
    setState({ busy: true, done: false, problem: '' });
    try { await send(n); setState({ busy: false, done: true, problem: '' }); } catch (e) { setState({ busy: false, done: false, problem: errorText(e) }); }
  };
  return (
    <form className="raise-limit" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      <input type="number" min={0} step="any" aria-label={`New limit for ${label}`} value={value} onChange={(e) => setValue(e.target.value)} />
      <Btn type="submit" size="sm" variant="primary" disabled={state.busy}>{continues ? 'Raise and continue' : 'Raise'}</Btn>
      <Btn size="sm" onClick={() => setOpen(false)}>Cancel</Btn>
      <small className="muted">For this run only.</small>
      {state.problem && <small className="tone-red" role="alert">{state.problem}</small>}
    </form>
  );
}
