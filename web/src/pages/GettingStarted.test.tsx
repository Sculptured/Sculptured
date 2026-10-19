import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { expect, it, vi } from 'vitest';
import { GettingStarted } from './GettingStarted';

const show = (p: Partial<Parameters<typeof GettingStarted>[0]> = {}) =>
  render(<MemoryRouter><GettingStarted hosted computer={false} cli={null} started={false} onHide={() => {}} {...p} /></MemoryRouter>);

it('hosted: three steps in order, each linking where it is done, with its state in words', () => {
  show();
  const steps = within(screen.getByRole('region', { name: 'Get your first team running' })).getAllByRole('listitem');
  expect(steps.map((s) => s.textContent)).toEqual([
    expect.stringMatching(/^○ To do.*Connect a computer/),
    expect.stringMatching(/^○ To do.*Sign in to Claude Code or Codex on it/),
    expect.stringMatching(/^○ To do.*Start an example team/),
  ]);
  expect(within(steps[0]).getByRole('link')).toHaveAttribute('href', '/computers');
  expect(within(steps[1]).getByRole('link')).toHaveAttribute('href', '/computers');
  expect(within(steps[2]).getByRole('link')).toHaveAttribute('href', '/new');
});

it('done steps say Done and lose their link; the next step is the one to do', () => {
  show({ computer: true, cli: true });
  const steps = screen.getAllByRole('listitem');
  expect(steps[0]).toHaveTextContent(/^✓ Done/);
  expect(within(steps[0]).queryByRole('link')).toBeNull();
  expect(steps[1]).toHaveTextContent(/^✓ Done/);
  expect(steps[2]).toHaveTextContent(/^○ To do/);
});

it('on your own computer there is nothing to connect, and signing in is on Account', () => {
  show({ hosted: false, computer: true });
  const steps = screen.getAllByRole('listitem');
  expect(steps).toHaveLength(2);
  expect(within(steps[0]).getByRole('link')).toHaveAttribute('href', '/account');
});

it('disappears once everything is done, and can be hidden before that', async () => {
  const { container } = show({ computer: true, cli: true, started: true });
  expect(container).toBeEmptyDOMElement();
  const onHide = vi.fn();
  show({ onHide });
  await userEvent.setup().click(screen.getByRole('button', { name: 'Hide' }));
  expect(onHide).toHaveBeenCalled();
});
