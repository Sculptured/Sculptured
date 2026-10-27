import { expect, it } from 'vitest';
import { teamDraft } from '../test-fixtures';
import { addRule, removeRule, setApproveWithin, testOfKind, testText, updateRule, withTestText } from './mandate';

it('rules are added, changed and removed, and an empty mandate leaves the team', () => {
  let d = addRule(teamDraft({}));
  expect(d.mandate?.rules).toEqual([{ id: 'rule-1', action: 'create_purchase_order', field: 'amount', test: { kind: 'at-most', value: 5000 } }]);
  d = addRule(d);
  d = updateRule(d, 'rule-2', { field: 'supplier', test: withTestText(testOfKind('one-of', { kind: 'at-most', value: 1 }), 'Acme, Bolt & Co ,') });
  expect(d.mandate?.rules?.[1].test).toEqual({ kind: 'one-of', values: ['Acme', 'Bolt & Co'] });
  expect(testText(d.mandate!.rules![1].test)).toBe('Acme, Bolt & Co');
  d = setApproveWithin(d, true);
  expect(d.mandate?.approve_within).toBe(true);
  d = setApproveWithin(removeRule(removeRule(d, 'rule-1'), 'rule-2'), false);
  expect('mandate' in d).toBe(false);
});

it('a limit typed with a dollar sign and commas is a number', () => {
  expect(withTestText({ kind: 'at-most', value: 0 }, '$12,500')).toEqual({ kind: 'at-most', value: 12500 });
  expect(testOfKind('present', { kind: 'at-most', value: 3 })).toEqual({ kind: 'present' });
  expect(testOfKind('at-least', { kind: 'at-most', value: 3 })).toEqual({ kind: 'at-least', value: 3 });
});
