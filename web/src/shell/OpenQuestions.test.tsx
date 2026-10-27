import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { OpenQuestions } from './OpenQuestions';
import type { QuestionView } from '../generated/QuestionView';

afterEach(() => vi.restoreAllMocks());

const question = (over: Partial<QuestionView>): QuestionView => ({
  id: 'q-1', seat: 's-1', rig: 'r-1', source: 'request-input',
  prompt: { header: null, text: 'Which colour?', options: [], allow_other: true, tool: null, tool_input: null },
  state: 'open', opened_at_ms: 0, deadline_ms: null, answer: null, answered_at_ms: null, late_answer: null,
  decision: null, withdrawn_reason: null, acknowledged: false, consequence: [], expired_seq: null, unconfirmed: null,
  ...over,
});

/** The server: GET seat questions answers `questions`; every POST is recorded with its body. */
function serve(questions: QuestionView[]) {
  const posts: { url: string; body: Record<string, unknown> }[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    if (init?.method === 'POST') posts.push({ url: String(url), body: JSON.parse(String(init.body)) });
    const body = init?.method === 'POST' ? questions[0] : questions;
    return Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } }));
  });
  return posts;
}

it('answers a question by choosing one of its options', async () => {
  const posts = serve([question({ prompt: { header: 'Palette', text: 'Which colour?', options: [{ label: 'Green', description: null }, { label: 'Blue', description: null }], allow_other: false, tool: null, tool_input: null } })]);
  render(<OpenQuestions seat="s-1" name="Planner" feed={[]} />);
  expect(await screen.findByText('Which colour?')).toBeInTheDocument();
  expect(screen.queryByRole('textbox')).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Green' }));
  expect(posts).toEqual([{ url: '/api/questions/q-1/answer', body: expect.objectContaining({ answer: { kind: 'text', text: 'Green' } }) }]);
});

it('answers an open question in words', async () => {
  const posts = serve([question({})]);
  render(<OpenQuestions seat="s-1" name="Planner" feed={[]} />);
  await userEvent.type(await screen.findByRole('textbox', { name: 'Your answer to Planner' }), 'Teal, please');
  await userEvent.click(screen.getByRole('button', { name: 'Send answer' }));
  expect(posts[0].body.answer).toEqual({ kind: 'text', text: 'Teal, please' });
});

it('refuses a tool approval, with a reason the agent is told', async () => {
  const posts = serve([question({ source: 'native-permission', prompt: { header: null, text: 'Allow Bash to run?', options: [], allow_other: false, tool: 'Bash', tool_input: { command: 'rm -rf build' } }, consequence: [{ code: 'delete', text: 'deletes files' }] })]);
  render(<OpenQuestions seat="s-1" name="Reviewer" feed={[]} />);
  expect(await screen.findByText('Reviewer wants to run Bash')).toBeInTheDocument();
  expect(screen.getByText('deletes files')).toBeInTheDocument();
  await userEvent.type(screen.getByRole('textbox', { name: 'Reason (optional)' }), 'Reviewers must not delete files');
  await userEvent.click(screen.getByRole('button', { name: 'Deny' }));
  expect(posts[0]).toEqual({ url: '/api/questions/q-1/answer', body: expect.objectContaining({ answer: { kind: 'deny', reason: 'Reviewers must not delete files' } }) });
});

it('denies an approval that was allowed automatically when nobody answered in time', async () => {
  const posts = serve([question({ source: 'native-permission', state: 'expired', prompt: { header: null, text: 'Allow Bash to run?', options: [], allow_other: false, tool: 'Bash', tool_input: {} } })]);
  render(<OpenQuestions seat="s-1" name="Reviewer" feed={[]} />);
  expect(await screen.findByText(/allowed automatically/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Deny now' }));
  expect(posts[0].body.answer).toEqual({ kind: 'deny', reason: null });
});

it('sends an answer again when it may not have reached the agent', async () => {
  const posts = serve([question({ state: 'answered', answer: { kind: 'text', text: 'Green' }, unconfirmed: 'no reply from the computer' })]);
  render(<OpenQuestions seat="s-1" name="Planner" feed={[]} />);
  expect(await screen.findByText(/no reply from the computer/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Send again' }));
  expect(posts[0].url).toBe('/api/questions/q-1/send-again');
});

it('shows nothing when no question needs the person', async () => {
  serve([question({ state: 'answered', answer: { kind: 'text', text: 'Green' } })]);
  const { container } = render(<OpenQuestions seat="s-1" name="Planner" feed={[]} />);
  await new Promise((r) => setTimeout(r, 20));
  expect(container).toBeEmptyDOMElement();
});

it('takes the focus when the person chose Answer', async () => {
  serve([question({})]);
  const { rerender } = render(<OpenQuestions seat="s-1" name="Planner" feed={[]} />);
  const box = await screen.findByRole('textbox', { name: 'Your answer to Planner' });
  expect(box).not.toHaveFocus();
  rerender(<OpenQuestions seat="s-1" name="Planner" feed={[]} focusToken={1} />);
  expect(box).toHaveFocus();
});

it('marks an urgent question as the agent says it is', async () => {
  serve([question({ prompt: { header: null, text: 'The build server is down: wait or skip?', options: [], allow_other: true, tool: null, tool_input: null, urgency: 'high' } })]);
  render(<OpenQuestions seat="s-1" name="Planner" feed={[]} />);
  expect(await screen.findByText('Urgent, says the agent')).toBeInTheDocument();
});

const change = {
  proposal_id: 'chg-1', system: 'ERP', reason: 'the press is down',
  payload: { amount: 7300, supplier: 'Acme Hydraulics', item: 'SP-118' },
  checks: [
    { rule_id: 'limit', field: 'amount', passed: false, actual: 7300, words: 'amount is 7300, at most 5000' },
    { rule_id: 'vendor', field: 'supplier', passed: true, actual: 'Acme Hydraulics', words: 'supplier is "Acme Hydraulics", one of Acme Hydraulics' },
  ],
};

it('a proposed change shows its fields and rule results, and is rejected with a reason', async () => {
  const posts = serve([question({ source: 'approval', deadline_ms: Date.now() + 60_000, prompt: { header: 'Approve a change', text: 'Order 2 SP-118 from Acme Hydraulics, $7300', options: [], allow_other: false, tool: 'create_purchase_order', tool_input: change } })]);
  render(<OpenQuestions seat="s-1" name="Buyer" feed={[]} />);
  expect(await screen.findByText('Buyer proposes a change')).toBeInTheDocument();
  expect(screen.getByText('Order 2 SP-118 from Acme Hydraulics, $7300')).toBeInTheDocument();
  expect(screen.getByRole('table', { name: 'The change, as it would be written' })).toHaveTextContent('supplierAcme Hydraulics');
  const rules = screen.getByRole('list', { name: "The team's rules (checked by Sculptured)" });
  expect(rules).toHaveTextContent('✗ fails: amount is 7300, at most 5000');
  expect(rules).toHaveTextContent('✓ passes: supplier is');
  expect(screen.getByText(/Not approved at .* if nobody answers/)).toBeInTheDocument();
  await userEvent.type(screen.getByRole('textbox', { name: 'Reason (optional)' }), 'Over budget');
  await userEvent.click(screen.getByRole('button', { name: 'Reject' }));
  expect(posts[0].body.answer).toEqual({ kind: 'deny', reason: 'Over budget' });
});

it('a proposed change is approved with Approve, and one that timed out says it was not approved', async () => {
  const posts = serve([question({ source: 'approval', prompt: { header: 'Approve a change', text: 'Order bearings', options: [], allow_other: false, tool: 'create_purchase_order', tool_input: change } })]);
  const { unmount } = render(<OpenQuestions seat="s-1" name="Buyer" feed={[]} />);
  await userEvent.click(await screen.findByRole('button', { name: 'Approve' }));
  expect(posts[0].body.answer).toEqual({ kind: 'allow' });
  unmount();
  serve([question({ source: 'approval', state: 'expired', prompt: { header: 'Approve a change', text: 'Order bearings', options: [], allow_other: false, tool: 'create_purchase_order', tool_input: change } })]);
  render(<OpenQuestions seat="s-1" name="Buyer" feed={[]} />);
  expect(await screen.findByText('Nobody answered in time, so it was not approved.')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Deny now' })).toBeNull();
});
