import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { CliAccounts } from './CliAccounts';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const clis = [
    { name: 'claude', installed: true, version: '2.1', signed_in: true, account: 'a@b.c', detail: '' },
    { name: 'codex', installed: true, version: '0.160', signed_in: false, account: null, detail: 'Not logged in' },
  ];
  const later = Math.floor(Date.now() / 1000) + 2 * 3600;
  const usage = [{ usage: { provider: 'codex', status: null, credits: '40', windows: [{ name: 'seven_day', used_percent: 96, resets_at: later }] },
    observed_ms: Date.now() - 5 * 60000, source: 'codex-session-files', seat: null }];
  return { ...m, api: { ...m.api, clis: vi.fn().mockResolvedValue(clis), planUsage: vi.fn().mockResolvedValue(usage), cliLogin: vi.fn().mockResolvedValue({ url: 'https://auth.example/x', output: '' }) } };
});

describe('CliAccounts', () => {
  it('says which CLI needs signing in and starts its sign-in', async () => {
    render(<CliAccounts />);
    const list = await screen.findByRole('list', { name: 'Agent accounts' });
    expect(list).toHaveTextContent('Claude: signed in (a@b.c)');
    expect(list).toHaveTextContent('Codex: not signed in');
    const codex = await screen.findByRole('list', { name: 'Codex plan usage' });
    expect(codex).toHaveTextContent(/weekly: 96% used · resets in 1 h 59 min|weekly: 96% used · resets in 2 h 0 min/);
    expect(codex).toHaveTextContent('new work held');
    expect(codex).toHaveTextContent('Credits: 40');
    expect(screen.getByRole('list', { name: 'Claude plan usage' })).toHaveTextContent('Plan usage unknown');
    await userEvent.setup().click(screen.getByRole('button', { name: 'Sign in' }));
    expect(api.cliLogin).toHaveBeenCalledWith('codex');
    expect(await screen.findByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', 'https://auth.example/x');
  });
});
