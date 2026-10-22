import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import type { TeamDraft } from '../generated/TeamDraft';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { draftAgent, teamDraft } from '../test-fixtures';
import type { EditResult } from '../useTeamDraft';
import { RunsOnField } from './AgentFields';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));
const SIGNED_IN: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x.com', role: 'member' }, csrf: 'c' };
const at = '2026-10-21T00:00:00Z';

function serve() {
  vi.spyOn(globalThis, 'fetch').mockImplementation((url) => String(url).endsWith('/gw/api/connections')
    ? reply({ connections: [
      { id: 'k1', name: 'Work', provider: 'anthropic-key', model: null, key_last4: '1234', checked: { ok: true, at, detail: '' }, created_at: at },
      { id: 'or1', name: 'Router', provider: 'openrouter', model: 'anthropic/claude-sonnet-4.5', key_last4: 'abcd', checked: { ok: false, at, detail: 'key revoked' }, created_at: at },
    ] })
    : reply({}, 404));
}

let latest: TeamDraft;
function Harness({ session, start }: { session: HostedSession; start: TeamDraft }) {
  const [draft, setDraft] = useState(start);
  latest = draft;
  const edit = (fn: (d: TeamDraft) => EditResult) => { const r = fn(draft); const next = 'agents' in r ? r : r.draft; latest = next; setDraft(next); return null; };
  return <MemoryRouter><HostedProvider value={session}><RunsOnField draft={draft} a={draft.agents[0]} edit={edit} locked={false} /></HostedProvider></MemoryRouter>;
}

const claudeTeam = () => teamDraft({ agents: [draftAgent('builder', { kind: 'claude' })] });

it('hosted: lists your model connections beside the subscription, and picking one sets kind and connection', async () => {
  serve();
  render(<Harness session={SIGNED_IN} start={claudeTeam()} />);
  const pick = await screen.findByRole('combobox', { name: 'Builder runs on' });
  await waitFor(() => expect(screen.getByRole('option', { name: 'Claude (API key) · Work' })).toBeInTheDocument());
  expect(screen.getByRole('option', { name: 'Claude (subscription)' })).toBeInTheDocument();
  expect(screen.getByRole('option', { name: /Claude via OpenRouter · Router/ })).toBeInTheDocument();
  expect(screen.getByRole('option', { name: 'Codex' })).toBeInTheDocument();
  expect(pick).toHaveValue('claude');
  await userEvent.selectOptions(pick, 'Claude (API key) · Work');
  expect(latest.agents[0]).toMatchObject({ kind: 'claude', connection: 'k1' });
  expect(pick).toHaveDisplayValue('Claude (API key) · Work');
  await userEvent.selectOptions(pick, 'Codex');
  expect(latest.agents[0].kind).toBe('codex');
  expect(latest.agents[0].connection ?? null).toBeNull();
});

it('a connection whose check failed shows a warning when chosen', async () => {
  serve();
  render(<Harness session={SIGNED_IN} start={teamDraft({ agents: [draftAgent('builder', { kind: 'claude', connection: 'or1' })] })} />);
  expect(await screen.findByText(/last check failed/)).toBeInTheDocument();
  expect(screen.getByText(/anthropic\/claude-sonnet-4\.5/)).toBeInTheDocument();
});

it('a removed connection is named as removed, not shown as the subscription', async () => {
  serve();
  render(<Harness session={SIGNED_IN} start={teamDraft({ agents: [draftAgent('builder', { kind: 'claude', connection: 'gone' })] })} />);
  expect(await screen.findByText(/connection was removed/)).toBeInTheDocument();
});

it('on your own computer the choice stays Claude or Codex', async () => {
  serve();
  render(<Harness session={{ kind: 'local' }} start={claudeTeam()} />);
  expect(screen.getByRole('radiogroup', { name: 'Builder runs on' })).toBeInTheDocument();
  expect(screen.queryByRole('combobox', { name: 'Builder runs on' })).toBeNull();
});
