import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { CliSignInSteps } from './CliSignInSteps';

afterEach(() => vi.restoreAllMocks());
const answer = (status: number, body: object) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));

it('Codex: open the page on any device and enter the one-time code shown here', async () => {
  const user = userEvent.setup();
  const copied: string[] = [];
  vi.spyOn(navigator.clipboard, 'writeText').mockImplementation((t) => { copied.push(t); return Promise.resolve(); });
  render(<CliSignInSteps cli="codex" started={{ url: 'https://auth.openai.com/codex/device', code: 'ABCD-12345', takes_code: false, output: '' }} />);
  expect(screen.getByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', 'https://auth.openai.com/codex/device');
  expect(screen.getByText('ABCD-12345')).toBeVisible();
  expect(screen.getByText(/any phone or computer/)).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Copy code' }));
  expect(copied).toEqual(['ABCD-12345']);
  expect(screen.queryByRole('textbox')).toBeNull();
});

it('Claude Code: the code its page shows is pasted here and sent to that computer', async () => {
  const calls: { url: string; body: unknown }[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    calls.push({ url: String(url), body: JSON.parse(String(init?.body ?? '{}')) });
    return answer(200, {});
  });
  const user = userEvent.setup();
  render(<CliSignInSteps cli="claude" host="pc-1" started={{ url: 'https://claude.ai/oauth/authorize?code=true', code: null, takes_code: true, output: '' }} />);
  expect(screen.getByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', 'https://claude.ai/oauth/authorize?code=true');
  await user.type(screen.getByRole('textbox', { name: 'Code from the sign-in page' }), ' abc#def ');
  await user.click(screen.getByRole('button', { name: 'Send code' }));
  expect(calls).toHaveLength(1);
  expect(calls[0].url).toContain('/api/clis/claude/login/code?host=pc-1');
  expect(calls[0].body).toMatchObject({ code: 'abc#def' });
  expect(await screen.findByText(/Code sent/)).toBeVisible();
});

it('a sign-in that has ended says so in plain words', async () => {
  vi.spyOn(globalThis, 'fetch').mockImplementation(() => answer(409, { error: { code: 'no_sign_in', message: 'internal words' } }));
  const user = userEvent.setup();
  render(<CliSignInSteps cli="claude" started={{ url: 'https://claude.ai/oauth/authorize', code: null, takes_code: true, output: '' }} />);
  await user.type(screen.getByRole('textbox', { name: 'Code from the sign-in page' }), 'abc');
  await user.click(screen.getByRole('button', { name: 'Send code' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('That sign-in has ended. Press Sign in to start again.');
});

it('no link printed: shows what the program said, as evidence', () => {
  render(<CliSignInSteps cli="codex" started={{ url: null, code: null, takes_code: false, output: 'error: something odd' }} />);
  expect(screen.getByText('error: something odd')).toHaveAttribute('data-raw-evidence');
  expect(screen.queryByRole('link')).toBeNull();
});
