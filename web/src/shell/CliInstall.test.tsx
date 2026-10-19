import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { CliInstall } from './CliInstall';

afterEach(() => vi.restoreAllMocks());

it("shows each system's official install command for a missing CLI, ready to copy", async () => {
  const user = userEvent.setup();
  const copied: string[] = [];
  vi.spyOn(navigator.clipboard, 'writeText').mockImplementation((t) => { copied.push(t); return Promise.resolve(); });
  render(<CliInstall cli="codex" />);
  const unix = screen.getByRole('group', { name: 'Mac or Linux' });
  expect(unix).toHaveTextContent('curl -fsSL https://chatgpt.com/codex/install.sh | sh');
  expect(screen.getByRole('group', { name: 'Windows' })).toHaveTextContent('irm https://chatgpt.com/codex/install.ps1 | iex');
  await user.click(within(unix).getByRole('button', { name: 'Copy' }));
  expect(copied).toEqual(['curl -fsSL https://chatgpt.com/codex/install.sh | sh']);
});

it("Claude Code's commands are Anthropic's", () => {
  render(<CliInstall cli="claude" />);
  expect(screen.getByRole('group', { name: 'Mac or Linux' })).toHaveTextContent('curl -fsSL https://claude.ai/install.sh | bash');
  expect(screen.getByRole('group', { name: 'Windows' })).toHaveTextContent('irm https://claude.ai/install.ps1 | iex');
});
