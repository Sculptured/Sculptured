import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentRunOn } from './RunOn';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } }));

function serve(settings: Record<string, unknown>) {
  const puts: Record<string, unknown>[] = [];
  let current = settings;
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    if (u.endsWith('/devices')) return reply([{ id: 'laptop', name: 'Laptop', connected: true }, { id: 'hpc', name: 'Delta login', connected: false }]);
    if (init?.method === 'PUT') {
      const body = JSON.parse(String(init.body)) as { settings: Record<string, unknown> };
      puts.push(body.settings);
      current = body.settings;
      return reply(body.settings);
    }
    return reply(current);
  });
  return puts;
}

it('runs an agent on another computer than the team, and back', async () => {
  const puts = serve({ posture: null, question_timeout_min: null, own_copies: false, host: 'laptop' });
  render(<AgentRunOn team="t1" agent="runner" name="Job runner" />);
  const select = await screen.findByLabelText('Job runner runs on');
  expect(select).toHaveValue('');
  expect(screen.getByRole('option', { name: 'Delta login (not connected)' })).toBeInTheDocument();
  await userEvent.selectOptions(select, 'hpc');
  await waitFor(() => expect(puts[0]).toMatchObject({ host: 'laptop', agent_hosts: { runner: 'hpc' } }));
  await userEvent.selectOptions(select, '');
  await waitFor(() => expect(puts[1].agent_hosts).toEqual({}));
});

it('says the team computer is the default', async () => {
  serve({ posture: null, question_timeout_min: null, own_copies: false, host: null, agent_hosts: { other: 'hpc' } });
  render(<AgentRunOn team="t1" agent="runner" name="Job runner" />);
  expect(await screen.findByRole('option', { name: "The team's computer" })).toBeInTheDocument();
});

it("names each agent's own computer for the board, and follows a change", async () => {
  const { useAgentComputers } = await import('./RunOn');
  const puts = serve({ posture: null, question_timeout_min: null, own_copies: false, host: 'laptop', agent_hosts: { runner: 'hpc' } });
  function Board() {
    const on = useAgentComputers('t1', true);
    return <p data-testid="on">{JSON.stringify(on)}</p>;
  }
  render(<><Board /><AgentRunOn team="t1" agent="other" name="Other" /></>);
  await waitFor(() => expect(screen.getByTestId('on')).toHaveTextContent('{"runner":"Delta login"}'));
  await userEvent.selectOptions(await screen.findByLabelText('Other runs on'), 'laptop');
  await waitFor(() => expect(puts).toHaveLength(1));
  await waitFor(() => expect(screen.getByTestId('on')).toHaveTextContent('"other":"Laptop"'));
});
