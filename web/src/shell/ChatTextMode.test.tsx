import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { ChatTextMode, ChatTextModeButton } from './ChatTextMode';
import { Composer, useAgentControls } from './AgentControls';
import { seatView } from '../test-fixtures';
import { api } from '../api';

vi.mock('../api', async orig => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, send: vi.fn().mockResolvedValue({ message_id: 'm1' }) } };
});
function Chat({ reply = 'Existing conversation', open = () => {} }: { reply?: string; open?: () => void }) {
  const c = useAgentControls(seatView('a'));
  return <><button>Outside chat</button><ChatTextMode className="agent-pane" name="Builder" onOpen={open}>
    <ChatTextModeButton /><div role="log">{reply}</div><Composer c={c} />
  </ChatTextMode></>;
}

it('expands the whole chat from the composer, retains mounted content and draft, and receives live updates', async () => {
  const user = userEvent.setup(), open = vi.fn();
  const { rerender } = render(<Chat open={open} />);
  const field = screen.getByLabelText('Message'), log = screen.getByRole('log');
  log.scrollTop = 42;
  await user.type(field, 'Unsent draft');
  const trigger = within(screen.getByRole('form')).getByRole('button', { name: 'Expand editor' });
  await user.click(trigger);
  const dialog = screen.getByRole('dialog', { name: 'Full screen · Builder' });
  expect(within(dialog).getByRole('log')).toBe(log);
  expect(within(dialog).getByLabelText('Message')).toBe(field);
  expect(field).toHaveValue('Unsent draft');
  expect(open).toHaveBeenCalledOnce();
  expect(screen.getByRole('button', { name: 'Outside chat' }).inert).toBe(true);
  rerender(<Chat reply="New live reply" open={open} />);
  expect(within(dialog).getByText('New live reply')).toBeInTheDocument();
  expect(log.scrollTop).toBe(42);
  await user.click(field);
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(field).toHaveValue('Unsent draft');
  await waitFor(() => expect(trigger).toHaveFocus());
});

it('sends from full-screen chat and exits without triggering outer navigation', async () => {
  const back = vi.fn(), user = userEvent.setup();
  render(<div onKeyDown={back}><Chat /></div>);
  await user.click(screen.getAllByRole('button', { name: 'Expand editor' })[0]);
  const dialog = screen.getByRole('dialog');
  await user.type(within(dialog).getByLabelText('Message'), 'A message{Shift>}{Enter}{/Shift}with a newline');
  await user.click(within(dialog).getByRole('button', { name: 'Send' }));
  expect(api.send).toHaveBeenCalledWith('dev.a@r', expect.objectContaining({ text: 'A message\nwith a newline' }), expect.any(String));
  fireEvent.keyDown(dialog, { key: 'Escape' });
  expect(back).not.toHaveBeenCalled();
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});

it('restores background interaction when the chat unmounts in Text Mode', async () => {
  const { unmount } = render(<Chat />);
  const outside = screen.getByRole('button', { name: 'Outside chat' });
  fireEvent.click(screen.getAllByRole('button', { name: 'Expand editor' })[0]);
  expect(outside.inert).toBe(true);
  unmount();
  expect(outside.inert).not.toBe(true);
  expect(document.body.style.overflow).not.toBe('hidden');
});
