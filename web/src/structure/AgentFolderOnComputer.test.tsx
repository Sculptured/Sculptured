import { render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { HostedSession } from '../hosted/session';
import { teamDraft } from '../test-fixtures';

const session = { kind: 'signed-in' } as HostedSession;
vi.mock('../hosted/HostedGate', () => ({ useHosted: () => session }));
const { FolderField } = await import('./AgentFields');
afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } }));

function serve(agentHosts: Record<string, string>) {
  vi.spyOn(globalThis, 'fetch').mockImplementation((url) => String(url).endsWith('/devices')
    ? reply([{ id: 'ncsa', name: 'NCSA', connected: true }, { id: 'desk', name: 'My Desktop', connected: true }])
    : reply({ posture: null, question_timeout_min: null, own_copies: false, host: 'desk', agent_hosts: agentHosts }));
}

it('an agent on its own computer shows its folder there, not the project folder on the team’s computer', async () => {
  const draft = { ...teamDraft(), folder: '/home/me/project' };
  const a = draft.agents[0];
  serve({ [a.id]: 'ncsa' });
  render(<FolderField draft={draft} a={{ ...a, folder: null }} edit={() => null} locked={false} />);
  expect(await screen.findByText('NCSA’s own folder')).toBeInTheDocument();
  expect(screen.getByText('Folder on NCSA')).toBeInTheDocument();
  expect(screen.queryByText(/from project/)).toBeNull();
});

it('an agent on the team’s computer keeps the project folder', async () => {
  const draft = { ...teamDraft(), folder: '/home/me/project' };
  const a = draft.agents[0];
  serve({});
  render(<FolderField draft={draft} a={{ ...a, folder: null }} edit={() => null} locked={false} />);
  expect(await screen.findByText(/from project/)).toBeInTheDocument();
});
