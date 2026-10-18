import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { expect, it } from 'vitest';
import type { QuestionView } from '../generated/QuestionView';
import { QuestionLog } from './QuestionLog';

const q = (id: string, over: Partial<QuestionView>): QuestionView => ({
  id, seat: 'a@t', rig: 't', source: 'request-input', state: 'answered', opened_at_ms: Date.UTC(2026, 9, 18, 14, 0), deadline_ms: null,
  prompt: { header: null, text: `Question ${id}?`, options: [], allow_other: true, tool: null, tool_input: null },
  answer: null, answered_at_ms: null, late_answer: null, decision: null, withdrawn_reason: null, acknowledged: false, consequence: [], expired_seq: null, unconfirmed: null, ...over,
});

const all: QuestionView[] = [
  q('open', { state: 'open', seat: 'b@t', deadline_ms: Date.UTC(2026, 9, 18, 15, 0), prompt: { header: null, text: 'Wait for the build server?', options: [], allow_other: true, tool: null, tool_input: null, urgency: 'high' } }),
  q('picked', { answer: { kind: 'text', text: 'Green' }, answered_at_ms: 1, prompt: { header: 'Palette', text: 'Which colour?', options: [{ label: 'Blue', description: null }, { label: 'Green', description: null }], allow_other: false, tool: null, tool_input: null } }),
  q('decided', { state: 'expired', acknowledged: true, answer: { kind: 'expired' }, decision: { text: 'Use Postgres', rationale: 'it is already running', source: 'record-decision', provenance: 'agent-reported', at_ms: 2 } }),
  q('gone', { state: 'withdrawn', withdrawn_reason: 'the agent stopped' }),
];

const names = { 'a@t': 'Planner', 'b@t': 'Builder' };

it('lists every question with its choices and what came of it, in words', () => {
  render(<MemoryRouter><QuestionLog team="t" questions={all} names={names} showAgent onChanged={() => {}} /></MemoryRouter>);
  const picked = screen.getByRole('listitem', { name: 'Palette' });
  expect(within(picked).getByRole('list', { name: 'Choices' }).textContent).toContain('Green (your answer)');
  expect(picked.textContent).toContain('You answered: Green');
  expect(screen.getByRole('listitem', { name: 'Question decided?' }).textContent).toContain('The agent decided: Use Postgres, because it is already running (agent-reported)');
  expect(screen.getByRole('listitem', { name: 'Question gone?' }).textContent).toContain('Withdrawn: the agent stopped');
  const open = screen.getByRole('listitem', { name: 'Wait for the build server?' });
  expect(open.textContent).toContain('Urgent, says the agent');
  expect(within(open).getByRole('link', { name: 'Builder' })).toHaveAttribute('href', '/p/t/agents/b');
  expect(within(open).getByRole('textbox')).toBeInTheDocument();
});

it('filters by what happened, with a count for each', async () => {
  render(<MemoryRouter><QuestionLog team="t" questions={all} names={names} showAgent onChanged={() => {}} /></MemoryRouter>);
  await userEvent.click(screen.getByRole('button', { name: 'Agent decided (1)' }));
  const entries = () => [...screen.getByRole('list', { name: 'Questions' }).children].map((li) => li.getAttribute('aria-label'));
  expect(entries()).toEqual(['Question decided?']);
  await userEvent.click(screen.getByRole('button', { name: 'Waiting for you (1)' }));
  expect(entries()).toEqual(['Wait for the build server?']);
});

it('says when there are none yet', () => {
  render(<MemoryRouter><QuestionLog team="t" questions={[]} names={{}} showAgent={false} onChanged={() => {}} /></MemoryRouter>);
  expect(screen.getByText(/No questions yet/)).toBeInTheDocument();
});
