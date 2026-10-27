import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { addResultCheck, removeResultCheck, updateResultCheck } from '../draft/mandate';
import type { TeamDraft } from '../generated/TeamDraft';
import { teamDraft } from '../test-fixtures';
import { ResultCheckFields } from './ResultCheckFields';

it('result checks are added, changed and removed, and none leaves the field out', () => {
  let list = addResultCheck([]).result_checks!;
  expect(list).toEqual([{ id: 'check-1', field: '', test: { kind: 'present' } }]);
  list = updateResultCheck(list, 'check-1', { field: 'price', test: { kind: 'at-most', value: 5000 } }).result_checks!;
  expect(list[0]).toEqual({ id: 'check-1', field: 'price', test: { kind: 'at-most', value: 5000 } });
  expect(removeResultCheck(list, 'check-1')).toEqual({ result_checks: undefined });
});

it('edits an agent\'s result checks in place', () => {
  const base = teamDraft({});
  const a = { ...base.agents[0], advanced: { ...base.agents[0].advanced, result_checks: [{ id: 'check-1', field: 'price', test: { kind: 'at-most' as const, value: 5000 } }] } };
  let draft: TeamDraft = { ...base, agents: [a, ...base.agents.slice(1)] };
  const edit = (fn: (d: TeamDraft) => unknown) => { draft = fn(draft) as TeamDraft; return null; };
  render(<ResultCheckFields a={a} edit={edit} locked={false} />);
  expect(screen.getByRole('heading', { name: 'Result checks' })).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText('Result check 1: value'), { target: { value: '$7,500' } });
  expect(draft.agents[0].advanced.result_checks?.[0].test).toEqual({ kind: 'at-most', value: 7500 });
  fireEvent.click(screen.getByRole('button', { name: 'Add a result check' }));
  expect(draft.agents[0].advanced.result_checks).toHaveLength(2);
});
