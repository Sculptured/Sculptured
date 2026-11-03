import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetProviders } from '../tests/providers';
import type { Health, ProviderStatus } from '../tests/types';
import { PaneContext } from '../workspace/registry';
import type { ArenaCtx } from './data';
import { EnginePane } from './panes/Engine';
import { NewRun } from './panes/NewRun';

/** The health check's providers: OpenRouter without a key, OpenAI keyed, Ollama not asked. */
const FROM_HEALTH: ProviderStatus[] = [
  { id: 'openrouter', kind: 'openrouter', label: 'OpenRouter', available: false, checked: false, why: 'OPENROUTER_API_KEY is not set' },
  { id: 'openai', kind: 'openai', label: 'OpenAI', available: true, checked: false },
  { id: 'local:ollama', kind: 'local', label: 'ollama (local)', available: false, checked: false, why: 'not checked (GET /api/providers asks the server)' },
];
/** `/api/providers`: Ollama answered with its models. */
const CHECKED: ProviderStatus[] = [
  FROM_HEALTH[0],
  { ...FROM_HEALTH[1], checked: true, models: ['gpt-5', 'gpt-5-mini'] },
  { id: 'local:ollama', kind: 'local', label: 'ollama (local)', available: true, checked: true, models: ['llama3.3'] },
];
const HEALTH: Health = { claude: true, codex: false, open_available: true, providers: FROM_HEALTH };
const TEAM = { id: 't', name: 't', agents: [{ id: 'triage', name: 'Triage', kind: 'claude_code' as const, model: null, prompt: '' }] };

let providersReply: [number, unknown];
let posted: unknown[];

beforeEach(() => {
  localStorage.clear();
  resetProviders();
  providersReply = [500, { error: { code: 'arena_failed', message: 'failed', raw: 'boom' } }];
  posted = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    const path = url.replace('/api/arena', '');
    if (init?.method === 'POST' && path === '/runs') { posted.push(JSON.parse(String(init.body))); return new Response(JSON.stringify({ run_id: 'r9', branch_id: 'b9' })); }
    const [status, body] = path === '/providers' ? providersReply : path === '/team/default' ? [200, TEAM] : [404, { error: { code: 'arena_not_found', message: path } }];
    return new Response(JSON.stringify(body), { status });
  }));
});
afterEach(() => vi.unstubAllGlobals());

const ctx = { health: HEALTH, healthError: null, runs: { value: [], error: null }, world: { value: null, error: null }, usage: { value: null, error: null } } as unknown as ArenaCtx;

describe('Engine pane: model providers', () => {
  it('shows one row per provider with its reason; locals are not checked until Check answers', async () => {
    const user = userEvent.setup();
    render(<PaneContext.Provider value={ctx}><EnginePane /></PaneContext.Provider>);
    // The first ask failed: the health check's list stays, with the local server not checked.
    await screen.findByRole('alert');
    expect(screen.getByText('OpenRouter').nextSibling).toHaveTextContent('✕ Unavailable: OPENROUTER_API_KEY is not set');
    expect(screen.getByText('OpenAI').nextSibling).toHaveTextContent('✓ Ready');
    expect(screen.getByText('ollama (local)').nextSibling).toHaveTextContent(/^\? Not checked$/);
    expect(screen.queryByText('API mode')).toBeNull();
    providersReply = [200, CHECKED];
    await user.click(screen.getByRole('button', { name: 'Check' }));
    await waitFor(() => expect(screen.getByText('ollama (local)').nextSibling).toHaveTextContent('✓ Ready'));
    expect(screen.getByRole('button', { name: 'Check again' })).toBeEnabled();
  });
});

const newRun = () => render(
  <MemoryRouter><NewRun tasks={[{ id: '27', reason: 'Swap the boots.' } as never]} health={HEALTH} onError={() => {}} /></MemoryRouter>,
);

describe('Team editor: provider and model per API-mode agent', () => {
  it('offers ready providers, disables unavailable ones with their reason, and needs a model', async () => {
    providersReply = [200, CHECKED];
    const user = userEvent.setup();
    newRun();
    await user.click(await screen.findByRole('button', { name: /Edit team/ }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Agent' }), 'API mode');
    const provider = screen.getByRole('combobox', { name: 'Provider' });
    // An agent naming no provider is on OpenRouter, which stays listed so the team shows what it names.
    expect(provider).toHaveDisplayValue(/^OpenRouter/);
    expect(screen.getByText('Triage: OpenRouter is unavailable: OPENROUTER_API_KEY is not set')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Start run' })).toBeDisabled();
    await user.selectOptions(provider, 'OpenAI');
    expect(screen.getByRole('option', { name: 'OpenRouter (unavailable: OPENROUTER_API_KEY is not set)' })).toBeDisabled();
    expect(screen.getByText('Triage: Name a model for OpenAI')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Start run' })).toBeDisabled();
    const model = screen.getByRole('combobox', { name: 'Model' });
    expect([...document.querySelectorAll(`#${CSS.escape(model.getAttribute('list')!)} option`)].map((o) => o.getAttribute('value'))).toEqual(['gpt-5', 'gpt-5-mini']);
    await user.type(model, ' gpt-5 ');
    await user.click(screen.getByRole('button', { name: 'Start run' }));
    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]).toMatchObject({ task_id: '27', team: { agents: [{ id: 'triage', kind: 'open', provider: 'openai', model: 'gpt-5' }] } });
  });

  it('sends no provider for a CLI agent', async () => {
    const user = userEvent.setup();
    localStorage.setItem('sculptured.tests.team.v1', JSON.stringify({ ...TEAM, agents: [{ ...TEAM.agents[0], provider: 'openai' }] }));
    newRun();
    await user.click(await screen.findByRole('button', { name: 'Start run' }));
    await waitFor(() => expect(posted).toHaveLength(1));
    expect((posted[0] as { team: { agents: object[] } }).team.agents[0]).not.toHaveProperty('provider');
    expect(within(screen.getByRole('region', { name: 'New run' })).getByText('(Claude Code)')).toBeInTheDocument();
  });
});
