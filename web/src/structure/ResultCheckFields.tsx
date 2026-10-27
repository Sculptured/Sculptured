import { useId } from 'react';
import { setAdvanced } from '../draft/edits';
import { addResultCheck, removeResultCheck, testOfKind, testText, updateResultCheck, withTestText, type TestKind } from '../draft/mandate';
import type { DraftAgent } from '../generated/DraftAgent';
import type { ResultCheck } from '../generated/ResultCheck';
import { Btn } from '../shell/ui';
import { TESTS } from './MandateFields';
import { LOCK_REASON, type Edit } from './Panels';

type Props = { a: DraftAgent; edit: Edit; locked: boolean };

function CheckRow({ c, n, list, a, edit, locked }: Props & { c: ResultCheck; n: number; list: ResultCheck[] }) {
  const set = (patch: Partial<Omit<ResultCheck, 'id'>>) => edit((d) => setAdvanced(d, a.id, updateResultCheck(list, c.id, patch)), `result-check:${a.id}:${c.id}`);
  const label = `Result check ${n}`;
  return (
    <div className="mandate-rule" role="group" aria-label={label}>
      <input className="fld-input mono" aria-label={`${label}: field`} value={c.field} disabled={locked} placeholder="part_number" spellCheck={false} onChange={(e) => set({ field: e.target.value })} />
      <select className="fld-input" aria-label={`${label}: test`} value={c.test.kind} disabled={locked} onChange={(e) => set({ test: testOfKind(e.target.value as TestKind, c.test) })}>
        {TESTS.map((t) => <option key={t.kind} value={t.kind}>{t.label}</option>)}
      </select>
      {c.test.kind !== 'present' && (
        <input className="fld-input" aria-label={`${label}: value`} value={testText(c.test)} disabled={locked} placeholder={c.test.kind === 'one-of' ? 'Acme Hydraulics, Bolt & Co' : '5000'}
          onChange={(e) => set({ test: withTestText(c.test, e.target.value) })} />
      )}
      <Btn size="sm" disabled={locked} aria-label={`Remove result check ${n}`} onClick={() => edit((d) => setAdvanced(d, a.id, removeResultCheck(list, c.id)))}>Remove</Btn>
    </div>
  );
}

/**
 * An agent's result checks: fields of the JSON block its result ends with, checked by Sculptured before
 * its work is handed on. A result that fails is sent back to the agent.
 */
export function ResultCheckFields({ a, edit, locked }: Props) {
  const ids = useId();
  const list = a.advanced.result_checks ?? [];
  return (
    <section className="fld" aria-labelledby={`${ids}-title`}>
      <h3 id={`${ids}-title`} className="fld-label">Result checks</h3>
      <p className="muted fld-hint">
        Before this agent's work is handed on, its result must end with a JSON block that passes these. A result that fails is sent back to it, up to three times.
      </p>
      {list.map((c, i) => <CheckRow key={c.id} c={c} n={i + 1} list={list} a={a} edit={edit} locked={locked} />)}
      <p><Btn size="sm" disabled={locked || list.length >= 16} onClick={() => edit((d) => setAdvanced(d, a.id, addResultCheck(list)))}>Add a result check</Btn></p>
      {locked && <p className="reason">{LOCK_REASON}</p>}
    </section>
  );
}
