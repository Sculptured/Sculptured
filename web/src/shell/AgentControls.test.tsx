import { WORDS } from '../vocab';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import type { RefObject } from 'react';
import { AgentActions, Composer, useAgentControls } from './AgentControls';
import { seatView } from '../test-fixtures';
import type { SeatView } from '../generated/SeatView';

/** The pane's controls: header actions plus the composer, sharing one controls hook. */
function SeatControls({ seat, name, fallbackFocus }: { seat: SeatView; name: string; fallbackFocus?: RefObject<HTMLElement | null> }) {
  const c = useAgentControls(seat, fallbackFocus);
  return (
    <>
      <AgentActions c={c} name={name} />
      <Composer c={c} />
    </>
  );
}

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, send: vi.fn(), interrupt: vi.fn(), controlAgent: vi.fn(), agentControl: vi.fn().mockResolvedValue({mode:null}) } };
});

describe('Agent controls', () => {
  beforeEach(() => vi.clearAllMocks());

  it('explains why Interrupt is disabled', () => {
    const { rerender } = render(<SeatControls name="a" seat={seatView('a')} />);
    expect(screen.getByRole('button', { name: 'Interrupt' })).toBeDisabled();
    expect(screen.getByText('No turn in progress')).toBeInTheDocument();

    // What the agent can do is unknown until its process reports it: said so, not guessed.
    rerender(<SeatControls name="a" seat={seatView('a', { current_turn: 't1', caps: null })} />);
    expect(screen.getByRole('button', { name: 'Interrupt' })).toBeDisabled();
    expect(screen.getByText(WORDS.interruptUnknown)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Interrupt' })).toHaveAccessibleDescription(WORDS.interruptUnknown);

    const caps = { ...seatView('a').caps!, interrupt: false };
    rerender(<SeatControls name="a" seat={seatView('a', { current_turn: 't1', caps })} />);
    expect(screen.getByRole('button', { name: 'Interrupt' })).toHaveAccessibleDescription(WORDS.cannotInterrupt);
    expect(WORDS.cannotInterrupt).toBe("This kind of agent can't be interrupted; wait for its turn to end, or stop it.");
    expect(WORDS.interruptUnknown).toBe("Whether this agent can be interrupted is unknown until it reports what it can do.");
    expect(document.body.textContent).not.toMatch(/driver/i);
  });

  it('disables Send with a visible reason when the seat is not running', () => {
    render(<SeatControls name="a" seat={seatView('a', { lifecycle: 'stopped' })} />);
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    expect(screen.getByText('Status: stopped; messages can only be sent to a running agent.')).toBeInTheDocument();
  });

  it('only allows the interrupt checkbox for instruct with a turn in progress', async () => {
    render(<SeatControls name="a" seat={seatView('a', { current_turn: 't1' })} />);
    // No sentence row under the box: how it is delivered sits behind ⋯ and defaults to inform.
    expect(screen.queryByText('delivered when its turn ends')).toBeNull();
    expect(screen.queryByRole('checkbox', { name: 'Interrupt current turn' })).toBeNull();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Message options (intent: inform)' }));
    const box = screen.getByRole('checkbox', { name: 'Interrupt current turn' });
    expect(box).toBeDisabled();
    expect(screen.getByRole('radio', { name: /inform\s*— delivered when its turn ends/ })).toBeChecked();
    await user.click(screen.getByRole('radio', { name: /instruct/ }));
    expect(box).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Message options (intent: instruct)' })).toBeInTheDocument();
    // A choice that is not the default is named beside Send.
    expect(screen.getByText('instruct', { selector: '.intent-chip' })).toBeInTheDocument();
    await user.click(box);
    expect(screen.getByText('interrupts now', { selector: '.intent-chip' })).toBeInTheDocument();
  });

  it('has no "You ›" prefix and no hint line; an empty box only tells screen readers why Send is off', () => {
    render(<SeatControls name="a" seat={seatView('a')} />);
    const form = screen.getByRole('form', { name: 'Send a message' });
    expect(form).not.toHaveTextContent('You ›');
    expect(screen.getByRole('button', { name: 'Send' })).toHaveAccessibleDescription('Type a message to send.');
    expect(screen.getByText('Type a message to send.')).toHaveClass('sr-only');
  });

  it('requires an inline confirm with the consequence before stopping', async () => {
    vi.mocked(api.controlAgent).mockResolvedValue({});
    render(<SeatControls name="a" seat={seatView('a', { current_turn: 't1' })} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Hard stop…' }));
    expect(api.controlAgent).not.toHaveBeenCalled();
    expect(
      screen.getByText('Stops a. 1 turn in progress will be cut off. Messages it has not read yet will be marked as not delivered.'),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Confirm stop agent' }));
    expect(api.controlAgent).toHaveBeenCalledWith('dev.a@r', 'hard', expect.any(String));
  });

  it('reuses the request id on a double submit and renews it after success', async () => {
    let resolve!: (v: { message_id: string }) => void;
    vi.mocked(api.send).mockImplementation(() => new Promise((r) => (resolve = r)));
    render(<SeatControls name="a" seat={seatView('a')} />);
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'hello' } });
    const form = screen.getByRole('form', { name: 'Send a message' });
    fireEvent.submit(form);
    fireEvent.submit(form);
    const ids = vi.mocked(api.send).mock.calls.map((c) => c[2]);
    expect(ids.length).toBeGreaterThanOrEqual(1);
    expect(new Set(ids).size).toBe(1);
    expect(vi.mocked(api.send).mock.calls[0][1]).toEqual({ text: 'hello', intent: 'inform', interrupt: false });

    resolve({ message_id: 'm1' });
    await screen.findByDisplayValue('');
    vi.mocked(api.send).mockResolvedValue({ message_id: 'm2' });
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'again' } });
    fireEvent.submit(form);
    const last = vi.mocked(api.send).mock.calls.at(-1)!;
    expect(last[2]).not.toBe(ids[0]);
  });

  it('renews the request id after the server rejects a send', async () => {
    vi.mocked(api.send).mockRejectedValueOnce(new ApiError(502, 'host_unreachable', "Sculptured's agent runner is not connected"));
    render(<SeatControls name="a" seat={seatView('a')} />);
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'hello' } });
    const form = screen.getByRole('form', { name: 'Send a message' });
    fireEvent.submit(form);
    expect(await screen.findByRole('alert')).toHaveTextContent("Sculptured's agent runner is not connected");
    vi.mocked(api.send).mockResolvedValue({ message_id: 'm2' });
    fireEvent.submit(form);
    const calls = vi.mocked(api.send).mock.calls;
    expect(calls).toHaveLength(2);
    expect(calls[1][2]).not.toBe(calls[0][2]);
  });

  it('moves focus to Cancel on open, back to Stop on Escape', async () => {
    render(<SeatControls name="a" seat={seatView('a')} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Hard stop…' }));
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('button', { name: 'Confirm stop agent' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Hard stop…' })).toHaveFocus();
  });

  it('disables Interrupt while in flight and reuses its id', async () => {
    vi.mocked(api.interrupt).mockImplementation(() => new Promise(() => {}));
    render(<SeatControls name="a" seat={seatView('a', { current_turn: 't1' })} />);
    const btn = screen.getByRole('button', { name: 'Interrupt' });
    fireEvent.click(btn);
    expect(await screen.findByText('Interrupting…')).toBeInTheDocument();
    expect(btn).toBeDisabled();
    expect(api.interrupt).toHaveBeenCalledTimes(1);
  });

  it('keeps the send id after a network failure', async () => {
    vi.mocked(api.send).mockRejectedValueOnce(new TypeError('Failed to fetch'));
    render(<SeatControls name="a" seat={seatView('a')} />);
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'hello' } });
    const form = screen.getByRole('form', { name: 'Send a message' });
    fireEvent.submit(form);
    await screen.findByRole('alert');
    vi.mocked(api.send).mockResolvedValue({ message_id: 'm2' });
    fireEvent.submit(form);
    const calls = vi.mocked(api.send).mock.calls;
    expect(calls[1][2]).toBe(calls[0][2]);
  });

  it('explains why the interrupt checkbox is disabled', async () => {
    const { rerender } = render(<SeatControls name="a" seat={seatView('a', { current_turn: 't1' })} />);
    await userEvent.setup().click(screen.getByRole('button', { name: 'Message options (intent: inform)' }));
    const box = screen.getByRole('checkbox', { name: 'Interrupt current turn' });
    expect(box).toHaveAccessibleDescription('Choose instruct to interrupt');
    expect(screen.getByText('Choose instruct to interrupt')).toBeInTheDocument();
    rerender(<SeatControls name="a" seat={seatView('a')} />);
    expect(box).toHaveAccessibleDescription('No turn in progress');
    rerender(<SeatControls name="a" seat={seatView('a', { current_turn: 't1', caps: null })} />);
    expect(box).toHaveAccessibleDescription(WORDS.interruptUnknown);
  });

  it('keeps text typed while a send is in flight', async () => {
    let resolve!: (v: { message_id: string }) => void;
    vi.mocked(api.send).mockImplementation(() => new Promise((r) => (resolve = r)));
    render(<SeatControls name="a" seat={seatView('a')} />);
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'first' } });
    fireEvent.submit(screen.getByRole('form', { name: 'Send a message' }));
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'first and more' } });
    resolve({ message_id: 'm1' });
    await vi.waitFor(() => expect(screen.getByText('Send')).toBeEnabled());
    expect(screen.getByLabelText('Message')).toHaveValue('first and more');
  });

  it('moves focus to the fallback after a successful stop disables Stop', async () => {
    vi.mocked(api.controlAgent).mockResolvedValue({});
    const ref = { current: null as HTMLElement | null };
    const Host = ({ seat }: { seat: ReturnType<typeof seatView> }) => (
      <>
        <h2 ref={(el) => void (ref.current = el)} tabIndex={-1}>
          heading
        </h2>
        <SeatControls name="a" seat={seat} fallbackFocus={ref} />
      </>
    );
    const { rerender } = render(<Host seat={seatView('a')} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Hard stop…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop agent' }));
    await screen.findByRole('button', { name: 'Hard stop…' });
    rerender(<Host seat={seatView('a', { lifecycle: 'stopped' })} />);
    expect(screen.getByRole('heading', { name: 'heading' })).toHaveFocus();
  });

  // M6: an attention seat may still have a process (or need settling), so it can be stopped.
  it('allows Stop for a seat that needs attention', async () => {
    vi.mocked(api.controlAgent).mockResolvedValue({});
    render(<SeatControls name="a" seat={seatView('a', { lifecycle: 'attention', lifecycle_reason: 'did not become ready within 60s' })} />);
    const stop = screen.getByRole('button', { name: 'Hard stop…' });
    expect(stop).toBeEnabled();
    const user = userEvent.setup();
    await user.click(stop);
    await user.click(screen.getByRole('button', { name: 'Confirm stop agent' }));
    expect(api.controlAgent).toHaveBeenCalledWith('dev.a@r', 'hard', expect.any(String));
  });

  it('says why Stop is disabled in terms of the seat status', () => {
    const { rerender } = render(<SeatControls name="a" seat={seatView('a', { lifecycle: 'stopped' })} />);
    expect(screen.getByRole('button', { name: 'Hard stop…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Hard stop…' })).toHaveAccessibleDescription('Status: stopped; there is no process to stop.');
    rerender(<SeatControls name="a" seat={seatView('a', { lifecycle: 'stopping' })} />);
    expect(screen.getByRole('button', { name: 'Hard stop…' })).toHaveAccessibleDescription('This agent is already stopping.');
  });
});

it('Ctrl+C and Ctrl+V in the composer never interrupt or suppress native clipboard actions', () => {
  vi.clearAllMocks();
  render(<SeatControls name="a" seat={seatView('a', { current_turn: 't1' })} />);
  const input = screen.getByLabelText('Message');
  expect(fireEvent.keyDown(input, { key: 'c', ctrlKey: true })).toBe(true);
  expect(fireEvent.keyDown(input, { key: 'v', ctrlKey: true })).toBe(true);
  expect(api.interrupt).not.toHaveBeenCalled();
});

it('requests a bounded wrap-up and shows the pending state',async()=>{
  vi.mocked(api.controlAgent).mockResolvedValue({});
  render(<SeatControls name="a" seat={seatView('a')} />);
  await userEvent.setup().click(screen.getByRole('button',{name:'Soft stop · ~5K'}));
  expect(api.controlAgent).toHaveBeenCalledWith('dev.a@r','soft',expect.any(String));
  expect(screen.getByRole('button',{name:'Wrapping up…'})).toBeDisabled();
});
