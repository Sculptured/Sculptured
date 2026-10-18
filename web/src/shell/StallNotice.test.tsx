import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { seatView } from '../test-fixtures';
import { NUDGE_TEXT, SILENT_NUDGE_TEXT, StallNotice } from './StallNotice';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, send: vi.fn().mockResolvedValue({ message_id: 'm' }) } };
});

describe('StallNotice', () => {
  it('shows the repeated error and nudges only when asked', async () => {
    const seat = seatView('a', { lifecycle: 'running', stalled: { reason: 'the same tool error 3 times in a row: boom', at_ms: 1 } });
    render(<StallNotice seat={seat} />);
    expect(screen.getByRole('region', { name: 'Agent may be stalled' })).toHaveTextContent('boom');
    expect(api.send).not.toHaveBeenCalled();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Nudge' }));
    expect(api.send).toHaveBeenCalledWith(seat.id, { text: NUDGE_TEXT, intent: 'instruct', interrupt: true });
  });

  it('nudges a silent agent to say where it is, not to change approach', async () => {
    const seat = seatView('b', { lifecycle: 'running', stalled: { reason: 'no output for 31 minutes in its turn, while running Bash', at_ms: 1, silent: true } });
    render(<StallNotice seat={seat} />);
    expect(screen.getByRole('region', { name: 'Agent may be stalled' })).toHaveTextContent('31 minutes');
    await userEvent.setup().click(screen.getByRole('button', { name: 'Nudge' }));
    expect(api.send).toHaveBeenLastCalledWith(seat.id, { text: SILENT_NUDGE_TEXT, intent: 'instruct', interrupt: true });
  });
});
