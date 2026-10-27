import { render, screen, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { ProposedChangeRow } from '../api';
import { ChangesView, decisionWords } from './ChangesView';

afterEach(() => vi.restoreAllMocks());

const row = (over: Partial<ProposedChangeRow>): ProposedChangeRow => ({
  id: 'chg-1', at_ms: Date.now(), agent: 'team.buyer@t', agent_name: 'Buyer', action: 'create_purchase_order', system: 'ERP',
  summary: 'Order 2 SP-118 from Acme Hydraulics, $4200', payload: { amount: 4200 }, reason: null,
  checks: [{ rule_id: 'limit', field: 'amount', passed: true, actual: 4200, words: 'amount is 4200, at most 5000' }],
  ...over,
});

it('says what became of each change, in words and a mark', () => {
  expect(decisionWords(row({}))).toMatchObject({ mark: '◌', words: 'Waiting for a person in Needs you' });
  expect(decisionWords(row({ decision: { approved: true, by: 'mandate', note: null, at_ms: 1 } })).words).toBe("Approved by the team's rules");
  expect(decisionWords(row({ decision: { approved: true, by: 'person', note: null, at_ms: 1 } })).words).toBe('Approved by a person');
  expect(decisionWords(row({ decision: { approved: false, by: 'person', note: 'Over budget', at_ms: 1 } })).words).toBe('Not approved: Over budget');
  expect(decisionWords(row({ decision: { approved: false, by: 'nobody-in-time', note: null, at_ms: 1 } })).words).toBe('Not approved: nobody answered in time');
});

it('lists the team\'s changes, newest first, with their rules', async () => {
  const changes = [row({ id: 'b', summary: 'Second', decision: { approved: true, by: 'mandate', note: null, at_ms: 2 } }), row({ id: 'a', summary: 'First' })];
  vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({ changes, mandate: null }), { status: 200, headers: { 'content-type': 'application/json' } }));
  render(<ChangesView rig="t" />);
  const items = await screen.findAllByRole('listitem');
  const rows = items.filter((li) => li.classList.contains('change-row'));
  expect(rows.map((r) => r.querySelector('strong')?.textContent)).toEqual(['Second', 'First']);
  expect(rows[0]).toHaveTextContent("✓ Approved by the team's rules");
  expect(within(rows[0]).getByRole('list', { name: "The team's rules (checked by Sculptured)" })).toHaveTextContent('amount is 4200, at most 5000');
});
