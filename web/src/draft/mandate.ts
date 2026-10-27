import type { AgentAdvanced } from '../generated/AgentAdvanced';
import type { ChangeRule } from '../generated/ChangeRule';
import type { ResultCheck } from '../generated/ResultCheck';
import type { Mandate } from '../generated/Mandate';
import type { RuleTest } from '../generated/RuleTest';
import type { TeamDraft } from '../generated/TeamDraft';

export type TestKind = RuleTest['kind'];

/** A mandate with nothing in it is left out of the team, as the server leaves it out of a team file. */
const withMandate = (d: TeamDraft, m: Mandate): TeamDraft => {
  const empty = (m.rules?.length ?? 0) === 0 && !m.approve_within;
  if (!empty) return { ...d, mandate: m };
  const { mandate: _gone, ...rest } = d;
  return rest;
};

const rulesOf = (d: TeamDraft) => d.mandate?.rules ?? [];

export const setApproveWithin = (d: TeamDraft, on: boolean): TeamDraft =>
  withMandate(d, { ...d.mandate, approve_within: on || undefined });

/** A new rule: purchase orders up to an amount, the most common first rule. */
export function addRule(d: TeamDraft): TeamDraft {
  const taken = new Set(rulesOf(d).map((r) => r.id));
  let n = rulesOf(d).length + 1;
  while (taken.has(`rule-${n}`)) n++;
  const rule: ChangeRule = { id: `rule-${n}`, action: 'create_purchase_order', field: 'amount', test: { kind: 'at-most', value: 5000 } };
  return withMandate(d, { ...d.mandate, rules: [...rulesOf(d), rule] });
}

export const updateRule = (d: TeamDraft, id: string, patch: Partial<Omit<ChangeRule, 'id'>>): TeamDraft =>
  withMandate(d, { ...d.mandate, rules: rulesOf(d).map((r) => (r.id === id ? { ...r, ...patch } : r)) });

export const removeRule = (d: TeamDraft, id: string): TeamDraft =>
  withMandate(d, { ...d.mandate, rules: rulesOf(d).filter((r) => r.id !== id) });

/** A test of another kind, keeping what it can of the old one's value. */
export function testOfKind(kind: TestKind, old: RuleTest): RuleTest {
  const number = 'value' in old ? old.value : 0;
  if (kind === 'at-most' || kind === 'at-least') return { kind, value: number };
  if (kind === 'one-of') return { kind, values: 'values' in old ? old.values : [] };
  return { kind: 'present' };
}

/** The test's value as the person typed it: a number, or a comma-separated list. */
export const testText = (t: RuleTest): string => ('value' in t ? String(t.value) : 'values' in t ? t.values.join(', ') : '');

export function withTestText(t: RuleTest, text: string): RuleTest {
  if (t.kind === 'at-most' || t.kind === 'at-least') return { kind: t.kind, value: Number(text.replace(/[$,\s]/g, '')) || 0 };
  if (t.kind === 'one-of') return { kind: 'one-of', values: text.split(',').map((s) => s.trim()).filter(Boolean) };
  return t;
}

/** An agent's result checks, with one added, changed or removed (none leaves the field out). */
const withChecks = (list: ResultCheck[]): Pick<AgentAdvanced, 'result_checks'> => ({ result_checks: list.length ? list : undefined });

export function addResultCheck(list: ResultCheck[]): Pick<AgentAdvanced, 'result_checks'> {
  const taken = new Set(list.map((c) => c.id));
  let n = list.length + 1;
  while (taken.has(`check-${n}`)) n++;
  return withChecks([...list, { id: `check-${n}`, field: '', test: { kind: 'present' } }]);
}

export const updateResultCheck = (list: ResultCheck[], id: string, patch: Partial<Omit<ResultCheck, 'id'>>) =>
  withChecks(list.map((c) => (c.id === id ? { ...c, ...patch } : c)));

export const removeResultCheck = (list: ResultCheck[], id: string) => withChecks(list.filter((c) => c.id !== id));
