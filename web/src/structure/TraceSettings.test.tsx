import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { TraceSettings } from './TraceSettings';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));
const off = { enabled: false, endpoint: 'https://api.lmnr.ai/v1/traces', key_saved: false, content: 'metadata', last_error: null, last_sent_ms: null };

function serve(first: object, answer: (body: Record<string, unknown>) => object = (b) => ({ ...first, ...b, key: undefined, key_saved: typeof b.key === 'string' ? b.key !== '' : (first as typeof off).key_saved })) {
  const puts: Record<string, unknown>[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((_url, init) => {
    if (init?.method === 'PUT') {
      const body = JSON.parse(String(init.body)) as Record<string, unknown>;
      puts.push(body);
      return reply(answer(body));
    }
    return reply(first);
  });
  return puts;
}

it('turns sending on with a key, then shows only that a key is saved', async () => {
  const puts = serve(off);
  render(<TraceSettings teamId="t1" />);
  await userEvent.click(await screen.findByRole('checkbox', { name: /send this team's runs as traces/i }));
  await userEvent.type(screen.getByLabelText(/api key/i), 'lmnr-secret');
  await userEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(puts[0]).toMatchObject({ enabled: true, endpoint: 'https://api.lmnr.ai/v1/traces', key: 'lmnr-secret', content: 'metadata' });
  expect(await screen.findByText(/a key is saved/i)).toBeInTheDocument();
  expect(screen.getByLabelText(/api key/i)).toHaveValue('');
  expect(document.body.textContent).not.toContain('lmnr-secret');
});

it('keeps the saved key unless one is typed or it is removed', async () => {
  const puts = serve({ ...off, enabled: true, key_saved: true });
  render(<TraceSettings teamId="t1" />);
  await userEvent.click(await screen.findByRole('radio', { name: /also prompts and outputs/i }));
  await userEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(puts[0]).toMatchObject({ content: 'full' });
  expect(puts[0]).not.toHaveProperty('key');
  await userEvent.click(screen.getByRole('button', { name: /remove key/i }));
  expect(puts[1]).toMatchObject({ key: '' });
});

it('sends to another collector the team names', async () => {
  const puts = serve({ ...off, enabled: true });
  render(<TraceSettings teamId="t1" />);
  await userEvent.selectOptions(await screen.findByLabelText(/send to/i), 'other');
  const address = screen.getByLabelText(/collector address/i);
  await userEvent.clear(address);
  await userEvent.type(address, 'https://otel.example.com/v1/traces');
  await userEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(puts[0]).toMatchObject({ endpoint: 'https://otel.example.com/v1/traces' });
});

it('shows why the last send failed, and a refused address', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation((_u, init) => init?.method === 'PUT'
    ? reply({ error: { code: 'trace_endpoint_invalid', message: 'Use an https:// address, or http:// to this computer' } }, 400)
    : reply({ ...off, enabled: true, key_saved: true, last_error: 'api.lmnr.ai refused the API key (401 Unauthorized)' }));
  render(<TraceSettings teamId="t1" />);
  expect(await screen.findByText(/refused the API key/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(/https:\/\/ address/);
});
