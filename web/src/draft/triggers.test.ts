import { expect, it } from 'vitest';
import { teamDraft } from '../test-fixtures';
import { addTrigger, removeTrigger, updateTrigger } from './triggers';

it('triggers are added for the first agent, changed and removed, and none leaves the team', () => {
  let d = addTrigger(teamDraft({}));
  const first = d.agents[0]?.id ?? '';
  expect(d.triggers).toEqual([{ id: 'trigger-1', agent: first, message: '', starts_team: true }]);
  d = addTrigger(d);
  d = updateTrigger(d, 'trigger-2', { message: 'A machine alarm. Open a work order.', starts_team: false });
  expect(d.triggers?.[1]).toMatchObject({ id: 'trigger-2', message: 'A machine alarm. Open a work order.', starts_team: false });
  d = removeTrigger(removeTrigger(d, 'trigger-1'), 'trigger-2');
  expect('triggers' in d).toBe(false);
});
