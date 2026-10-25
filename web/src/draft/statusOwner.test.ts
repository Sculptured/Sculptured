import { expect, it } from 'vitest';
import type { DraftConnection } from '../generated/DraftConnection';
import { draftAgent, teamDraft } from '../test-fixtures';
import { setStatusOwner, statusOwner } from './statusOwner';

const hands = (from: string, to: string): DraftConnection => ({ id: `${from}-${to}`, from, to, kind: 'hands-work-to' });

it('the end of the line writes the status page, else the lead, else the first agent', () => {
  const chain = teamDraft({ agents: [draftAgent('builder'), draftAgent('reviewer')], connections: [hands('builder', 'reviewer')] });
  expect(statusOwner(chain)?.id).toBe('reviewer');
  const fan = teamDraft({ agents: [draftAgent('planner'), draftAgent('b1'), draftAgent('b2')], connections: [hands('planner', 'b1'), hands('planner', 'b2')] });
  expect(statusOwner(fan)?.id).toBe('planner');
  expect(statusOwner(teamDraft({ agents: [draftAgent('solo')], connections: [] }))?.id).toBe('solo');
  expect(statusOwner(teamDraft({ agents: [], connections: [] }))).toBeNull();
});

it('choosing an agent makes it the only one chosen; clearing goes back to the default', () => {
  const d = teamDraft({ agents: [draftAgent('builder'), draftAgent('reviewer')], connections: [hands('builder', 'reviewer')] });
  const picked = setStatusOwner(setStatusOwner(d, 'reviewer'), 'builder');
  expect(picked.agents.map((a) => !!a.advanced.writes_status)).toEqual([true, false]);
  expect('writes_status' in picked.agents[1].advanced).toBe(false);
  expect(statusOwner(picked)?.id).toBe('builder');
  expect(statusOwner(setStatusOwner(picked, null))?.id).toBe('reviewer');
});
