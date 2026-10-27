import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { TeamTriggers } from '../api';
import { teamDraft } from '../test-fixtures';
import { outcomeWords, TriggerFields } from './TriggerFields';

afterEach(() => vi.restoreAllMocks());

const json = (v: unknown) => new Response(JSON.stringify(v), { status: 200, headers: { 'content-type': 'application/json' } });

it('says what came of an event, in a mark and words', () => {
  const e = { id: 'evt-1', trigger: 'alarm', at_ms: 1, body: {} };
  expect(outcomeWords(e)).toBe('… being handled');
  expect(outcomeWords({ ...e, delivered: true, outcome: 'started the team and sent' })).toBe('✓ started the team and sent');
  expect(outcomeWords({ ...e, delivered: false, outcome: 'not delivered: the team is not running' })).toBe('✗ not delivered: the team is not running');
});

it('shows each trigger\'s full address and its latest events, and replaces the address only when confirmed', async () => {
  const base = teamDraft({});
  const draft = { ...base, triggers: [{ id: 'alarm', agent: base.agents[0].id, message: 'A machine alarm.', starts_team: true }] };
  const listed: TeamTriggers = {
    triggers: [{ id: 'alarm', path: `/hooks/u1/${'a'.repeat(64)}` }],
    received: [{ id: 'evt-1', trigger: 'alarm', at_ms: Date.now(), body: { machine: 'Press 4' }, delivered: true, outcome: 'sent' }],
  };
  const fetch = vi.spyOn(globalThis, 'fetch').mockImplementation(() => Promise.resolve(json(listed)));
  render(<TriggerFields draft={draft} edit={() => null} locked={false} />);
  expect(await screen.findByLabelText('Address')).toHaveTextContent(`${window.location.origin}/hooks/u1/${'a'.repeat(64)}`);
  expect(screen.getByText(/keep it like a password/)).toBeInTheDocument();
  const events = screen.getByRole('list', { name: 'Trigger 1: latest events' });
  expect(within(events).getByText(/✓ sent/)).toBeInTheDocument();
  expect(events).toHaveTextContent('Press 4');
  const posts = () => fetch.mock.calls.filter(([, init]) => init?.method === 'POST').length;
  fireEvent.click(screen.getByRole('button', { name: 'New address' }));
  expect(screen.getByText('The current address stops working.')).toBeInTheDocument();
  expect(posts()).toBe(0);
  fireEvent.click(screen.getByRole('button', { name: 'Replace it' }));
  await vi.waitFor(() => expect(posts()).toBe(1));
});

it('a trigger not saved yet has no address, and a running team cannot be changed', () => {
  const base = teamDraft({});
  vi.spyOn(globalThis, 'fetch').mockImplementation(() => Promise.resolve(json({ triggers: [], received: [] })));
  render(<TriggerFields draft={{ ...base, triggers: [{ id: 'x', agent: base.agents[0].id, message: '', starts_team: true }] }} edit={() => null} locked />);
  expect(screen.getByText('The address appears once this trigger is saved.')).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Add a trigger' })).toBeDisabled();
  expect(screen.getByText('Stop the team to change this.')).toBeInTheDocument();
});
