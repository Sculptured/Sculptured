import { act, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import { ERROR_WORDS } from '../errors';
import { ev, seatView, snapshot } from '../test-fixtures';
import { ActionError } from './source';
import { A, B, open, resetFakes, run, stream } from './testHarness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const fns = ['listRigs', 'latestSeatEvents', 'teamQuestions', 'answer', 'teamUsage', 'teamBudget', 'runUsage', 'teamControls', 'controlAgent', 'pauseTeam', 'resumeTeam', 'stopTeamNow', 'send'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(fns.map((f) => [f, vi.fn()])) } };
});

beforeEach(resetFakes);

type Answer = Awaited<ReturnType<typeof api.pauseTeam>>;

describe('deployment source controls', () => {
  it('pause and resume say why they cannot run, and name the agents a pause left alone', async () => {
    vi.mocked(api.teamControls).mockResolvedValue({ [B]: 'paused' });
    const { result } = await open(snapshot());
    await waitFor(() => expect(result.current.agentControl(B, 'pause').can).toEqual({ ok: false, why: 'Already paused.' }));
    expect(result.current.agents[1].status).toEqual({ glyph: '‖', word: 'paused', tone: 'idle' });
    expect(result.current.agentControl(B, 'resume').can).toEqual({ ok: true });
    expect(result.current.agentControl(A, 'resume').can).toEqual({ ok: false, why: 'Not paused, so there is nothing to resume.' });
    expect(result.current.agentControl(A, 'stop').can).toEqual({ ok: true });
    expect(result.current.message(A, '  ').can).toEqual({ ok: false, why: 'Type a message to send.' });
    expect(result.current.team.pauseAll.can).toEqual({ ok: true, note: 'Pauses 1 agent' });
    const answer = { affected: [A], skipped: [{ seat: B, code: 'already_paused' as const, reason: 'server words' }], notes: [{ seat: A, reason: 'A note' }] };
    vi.mocked(api.pauseTeam).mockResolvedValue(answer as Answer);
    let report: unknown;
    await act(async () => { report = await result.current.team.pauseAll.run(); });
    expect(report).toEqual({
      text: 'Paused 1 agent; 1 agent left as they were.',
      skipped: [{ agent: B, name: 'b', reason: 'Already paused.' }],
      notes: [{ agent: A, name: 'Builder', text: 'A note' }],
    });
  });

  it('resuming a paused agent reports what it resumed', async () => {
    vi.mocked(api.teamControls).mockResolvedValue({ [B]: 'paused' });
    vi.mocked(api.controlAgent).mockResolvedValue({ mode: 'resume', affected: [B], skipped: [], notes: [] });
    const { result } = await open(snapshot());
    await waitFor(() => expect(result.current.agentControl(B, 'resume').can).toEqual({ ok: true }));
    let report: unknown;
    await act(async () => { report = await result.current.agentControl(B, 'resume').run(); });
    expect(vi.mocked(api.controlAgent).mock.calls[0].slice(0, 2)).toEqual([B, 'resume']);
    expect(report).toEqual({ text: 'Resumed 1 agent.', skipped: [] });
  });

  it('a stop under way or any other control mode means already stopping; pausing has its own glyph', async () => {
    vi.mocked(api.teamControls).mockResolvedValue({ [A]: 'soft', [B]: 'pausing' });
    const { result } = await open(snapshot());
    await waitFor(() => expect(result.current.agentControl(A, 'pause').can).toEqual({ ok: false, why: 'Already stopping.' }));
    expect(result.current.agents[1].status).toEqual({ glyph: '⧗', word: 'pausing', tone: 'idle' });
    expect(result.current.team.pauseAll.can).toEqual({ ok: false, why: 'Already paused.' });
    vi.mocked(api.teamControls).mockResolvedValue({ [A]: 'budget' });
    act(() => stream().emit('event', { rig: 'r', event: ev(20, { type: 'seat-lifecycle-changed', lifecycle: 'running', reason: null }), seat: seatView('a') }));
    await waitFor(() => expect(result.current.team.pauseAll.can).toEqual({ ok: true, note: 'Pauses 1 agent' }));
    expect(result.current.agentControl(A, 'pause').can).toEqual({ ok: false, why: 'Already stopping.' });
  });

  it('control modes come from one team request, reloaded on a status change and after an action', async () => {
    const { result } = await open(snapshot());
    await waitFor(() => expect(api.teamControls).toHaveBeenCalledTimes(1));
    expect(api.teamControls).toHaveBeenCalledWith('r');
    act(() => stream().emit('event', { rig: 'r', event: ev(30, { type: 'seat-lifecycle-changed', lifecycle: 'stopping', reason: null }), seat: seatView('a', { lifecycle: 'stopping' }) }));
    await waitFor(() => expect(api.teamControls).toHaveBeenCalledTimes(2));
    vi.mocked(api.controlAgent).mockResolvedValue({ mode: 'hard' });
    await act(async () => { await result.current.agentControl(B, 'stop').run(); });
    expect(vi.mocked(api.controlAgent).mock.calls[0].slice(0, 2)).toEqual([B, 'hard']);
    await waitFor(() => expect(api.teamControls).toHaveBeenCalledTimes(3));
  });

  it('a refused action rejects with an ActionError in the plain words of errors.ts', async () => {
    vi.mocked(api.pauseTeam).mockRejectedValue(new ApiError(409, 'team_not_running', 'the server text'));
    const { result } = await open(snapshot());
    let caught: unknown;
    await act(async () => { await result.current.team.pauseAll.run().catch((e: unknown) => { caught = e; }); });
    expect(caught).toBeInstanceOf(ActionError);
    expect((caught as Error).message).toBe(ERROR_WORDS.team_not_running);
    // Not reachable at all: the plain words for that.
    vi.mocked(api.send).mockRejectedValue(new TypeError('Failed to fetch'));
    await expect(result.current.message(A, 'hello').run()).rejects.toThrow('Could not reach Sculptured');
  });

  it('messages are instructions, and Start asks the project bar when pressed', async () => {
    vi.mocked(api.send).mockResolvedValue({ message_id: 'm1' });
    const stopped = snapshot({ seats: [seatView('a', { lifecycle: 'stopped' }), seatView('b', { lifecycle: 'stopped' })] });
    const { result, rerender } = await open(stopped);
    expect(result.current.message(A, 'Check the tests').can).toEqual({ ok: false, why: 'Status: stopped; messages can only be sent to a running agent.' });
    const askStart = vi.fn();
    run.current = { live: false, startReason: 'Fix 2 things first', stopReason: null, askStart, askStop: vi.fn() };
    await expect(result.current.team.start.run()).resolves.toEqual({ text: 'Fix 2 things first', skipped: [], refused: true });
    // The bar's reason disables Start up front, once the source reads it again.
    rerender();
    expect(result.current.team.start.can).toEqual({ ok: false, why: 'Fix 2 things first' });
    run.current = { ...run.current, startReason: null };
    rerender();
    await act(async () => { await result.current.team.start.run(); });
    expect(askStart).toHaveBeenCalledTimes(1);
    act(() => stream().emit('snapshot', snapshot()));
    expect(result.current.team.start.can).toEqual({ ok: false, why: 'The team is already running.' });
    await act(async () => { await result.current.message(A, 'Check the tests').run(); });
    expect(vi.mocked(api.send).mock.calls[0].slice(0, 2)).toEqual([A, { text: 'Check the tests', intent: 'instruct', interrupt: false }]);
  });
});
