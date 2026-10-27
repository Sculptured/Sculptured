import { useId } from 'react';
import { addRule, removeRule, setApproveWithin, testOfKind, testText, updateRule, withTestText, type TestKind } from '../draft/mandate';
import type { ChangeRule } from '../generated/ChangeRule';
import type { TeamDraft } from '../generated/TeamDraft';
import { Btn } from '../shell/ui';
import { LOCK_REASON, type Edit } from './Panels';

/** The tests a rule can make, in words. Result checks use them too. */
export const TESTS: { kind: TestKind; label: string }[] = [
  { kind: 'at-most', label: 'is at most' },
  { kind: 'at-least', label: 'is at least' },
  { kind: 'one-of', label: 'is one of' },
  { kind: 'present', label: 'is filled in' },
];

type Props = { draft: TeamDraft; edit: Edit; locked: boolean };

/** One rule: for changes of an action, a field must pass a test. */
function RuleRow({ r, n, edit, locked }: { r: ChangeRule; n: number; edit: Edit; locked: boolean }) {
  const set = (patch: Partial<Omit<ChangeRule, 'id'>>) => edit((d) => updateRule(d, r.id, patch), `rule:${r.id}`);
  const label = `Rule ${n}`;
  return (
    <div className="mandate-rule" role="group" aria-label={label}>
      <span className="muted">For</span>
      <input className="fld-input mono" aria-label={`${label}: action (* for every change)`} value={r.action} disabled={locked} placeholder="*" spellCheck={false} onChange={(e) => set({ action: e.target.value })} />
      <input className="fld-input mono" aria-label={`${label}: field`} value={r.field} disabled={locked} placeholder="amount" spellCheck={false} onChange={(e) => set({ field: e.target.value })} />
      <select className="fld-input" aria-label={`${label}: test`} value={r.test.kind} disabled={locked} onChange={(e) => set({ test: testOfKind(e.target.value as TestKind, r.test) })}>
        {TESTS.map((t) => <option key={t.kind} value={t.kind}>{t.label}</option>)}
      </select>
      {r.test.kind !== 'present' && (
        <input className="fld-input" aria-label={`${label}: value`} value={testText(r.test)} disabled={locked} placeholder={r.test.kind === 'one-of' ? 'Acme Hydraulics, Bolt & Co' : '5000'}
          onChange={(e) => set({ test: withTestText(r.test, e.target.value) })} />
      )}
      <Btn size="sm" disabled={locked} aria-label={`Remove rule ${n}`} onClick={() => edit((d) => removeRule(d, r.id))}>Remove</Btn>
    </div>
  );
}

/**
 * Settings → Changes to business systems: the rules a change an agent proposes (`propose_change`) must
 * meet, checked by Sculptured itself, and whether a change that meets them is approved without asking.
 */
export function MandateFields({ draft, edit, locked }: Props) {
  const ids = useId();
  const rules = draft.mandate?.rules ?? [];
  return (
    <div className="mandate">
      <label className="fld-checkline">
        <input type="checkbox" checked={!!draft.mandate?.approve_within} disabled={locked} aria-describedby={`${ids}-hint`}
          onChange={(e) => edit((d) => setApproveWithin(d, e.target.checked))} />
        Approve a change that passes every rule below without asking
      </label>
      <p id={`${ids}-hint`} className="muted fld-hint">
        Anything else, and any change no rule applies to, waits for a person in Needs you; nobody answering means not approved.
      </p>
      {rules.map((r, i) => <RuleRow key={r.id} r={r} n={i + 1} edit={edit} locked={locked} />)}
      <p><Btn size="sm" disabled={locked} onClick={() => edit((d) => addRule(d))}>Add a rule</Btn></p>
      {locked && <p className="reason">{LOCK_REASON}</p>}
    </div>
  );
}
