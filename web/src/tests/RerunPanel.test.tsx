import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetProviders } from './providers';
import { RerunPanel } from './RerunPanel';
import type { AgentSpec, Change, ProviderStatus, RestoreMode } from './types';

const PROVIDERS: ProviderStatus[] = [
  { id: 'openrouter', kind: 'openrouter', label: 'OpenRouter', available: true, checked: false },
  { id: 'anthropic', kind: 'anthropic', label: 'Anthropic', available: false, checked: false, why: 'ANTHROPIC_API_KEY is not set' },
  { id: 'local:vllm', kind: 'local', label: 'vllm (local)', available: true, checked: true, models: ['qwen3'] },
];
const AGENTS: AgentSpec[] = [
  { id: 'triage', name: 'Triage', kind: 'open', model: 'deepseek/deepseek-chat-v3.1', prompt: 'Look.' },
  { id: 'resolver', name: 'Resolver', kind: 'codex', model: null, prompt: 'Fix.' },
];

let started: { changes: Record<string, Change>; mode: RestoreMode }[];

beforeEach(() => {
  resetProviders();
  started = [];
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(PROVIDERS))));
});
afterEach(() => vi.unstubAllGlobals());

const panel = () => render(
  <RerunPanel agents={AGENTS} seq={4} running={false} openAvailable providers={PROVIDERS} onClose={() => {}}
    plan={async () => ({ parts: [] })} start={async (changes, mode) => { started.push({ changes, mode }); }} />,
);

describe('Fork panel: provider and model', () => {
  it('sends a new provider with its model, and refuses to fork without a model or on an unavailable provider', async () => {
    const user = userEvent.setup();
    panel();
    const [provider] = await screen.findAllByRole('combobox', { name: 'Provider' });
    expect(screen.getByRole('option', { name: 'Anthropic (unavailable: ANTHROPIC_API_KEY is not set)' })).toBeDisabled();
    await user.selectOptions(provider, 'vllm (local)');
    const fork = screen.getByRole('button', { name: /^Fork with/ });
    expect(fork).toBeDisabled();
    expect(screen.getAllByText('Triage: Name a model for vllm (local)').length).toBeGreaterThan(0);
    const [model] = screen.getAllByRole('combobox', { name: 'Model' });
    expect(model).toHaveValue('');
    await user.type(model, 'qwen3');
    expect(fork).toBeEnabled();
    await user.click(fork);
    await waitFor(() => expect(started).toHaveLength(1));
    expect(started[0].changes).toEqual({ triage: { provider: 'local:vllm', model: 'qwen3' } });
  });

  it('sends no provider when only the model changes, or the provider is picked back', async () => {
    const user = userEvent.setup();
    panel();
    const [provider] = await screen.findAllByRole('combobox', { name: 'Provider' });
    await user.selectOptions(provider, 'vllm (local)');
    await user.selectOptions(provider, 'OpenRouter');
    const [model] = screen.getAllByLabelText('Model');
    expect(model).toHaveValue('deepseek/deepseek-chat-v3.1');
    await user.clear(model);
    await user.type(model, 'openai/gpt-5');
    await user.click(screen.getByRole('button', { name: 'Fork with 1 change' }));
    await waitFor(() => expect(started).toHaveLength(1));
    expect(started[0].changes).toEqual({ triage: { model: 'openai/gpt-5' } });
  });

  it('a CLI agent switched to API mode gets a provider choice; no provider unless one is picked', async () => {
    const user = userEvent.setup();
    panel();
    const resolverMode = (await screen.findAllByRole('combobox', { name: 'Agent' }))[1];
    await user.selectOptions(resolverMode, 'API mode');
    expect(screen.getAllByRole('combobox', { name: 'Provider' })).toHaveLength(2);
    await user.click(screen.getByRole('button', { name: /^Fork with/ }));
    await waitFor(() => expect(started).toHaveLength(1));
    expect(started[0].changes).toEqual({ resolver: { kind: 'open' } });
  });
});
