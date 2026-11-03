import { act, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import { ev, seatView, snapshot } from '../test-fixtures';
import { alarmsFrom, DEFAULT_RULES } from './alarms';
import { A, B, open, question, resetFakes, stream } from './testHarness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const fns = ['listRigs', 'latestSeatEvents', 'teamQuestions', 'answer', 'teamUsage', 'teamBudget', 'runUsage', 'teamControls', 'controlAgent', 'pauseTeam', 'resumeTeam', 'stopTeamNow', 'send'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(fns.map((f) => [f, vi.fn()])) } };
});

beforeEach(resetFakes);

describe('useDeploymentSource', () => {
  it('deployment_source_maps_agents_and_feed', async () => {
    const snap = snapshot({ seats: [seatView('a', { current_tool: 'Bash', tokens_in: 10, tokens_out: 20 }), seatView('b', { lifecycle: 'failed', lifecycle_reason: 'crashed' })] });
    const history = {
      [A]: [ev(1, { type: 'tool-called', turn_id: null, call_id: 'c1', name: 'Bash', input: { command: 'ls' }, parent_call_id: null }), ev(2, { type: 'stall-suspected', reason: 'same error' })],
      [B]: [ev(3, { type: 'seat-lifecycle-changed', lifecycle: 'failed', reason: 'crashed' }, B)],
    };
    const { result } = await open(snap, history);
    act(() => stream().emit('event', { rig: 'r', event: ev(4, { type: 'context-ceiling-reached', used_tokens: 900, ceiling: 1000 }), seat: null }));
    await waitFor(() => expect(result.current.feed).toHaveLength(4));
    const s = result.current;
    expect([s.kind, s.title, s.running]).toEqual(['deployment', 'My team', true]);
    expect(s.agents).toEqual([
      { id: A, name: 'Builder', status: { glyph: '●', word: 'running', tone: 'ok' }, activity: 'running a shell command', tokens: { in: 10, out: 20 } },
      { id: B, name: 'b', status: { glyph: '✕', word: 'failed', tone: 'bad' }, activity: 'failed', alarm: { rule: 'failed', key: `${B}:1` } },
    ]);
    expect(s.feed.map((f) => [f.id, f.agent, f.kind, f.text, f.alarm])).toEqual([
      ['1', A, 'tool-called', 'Builder ran a shell command: ls', undefined],
      ['2', A, 'stall-suspected', 'Builder may be stuck (suspected): same error', 'stall'],
      ['3', B, 'seat-lifecycle-changed', 'b failed: crashed', 'failed'],
      ['4', A, 'context-ceiling-reached', 'Builder reached its context limit (900 of 1000 tokens)', 'context-ceiling'],
    ]);
    expect(s.feed[0].raw).toEqual(history[A][0]);
    await waitFor(() => expect(result.current.metrics.tokensIn).toBe(100));
    expect(result.current.metrics).toEqual({ tokensIn: 100, tokensOut: 50, costUsd: 0.5, toolCallsPerMin: [...new Array(14).fill(0), 1] });
  });

  it('approvals_from_questions_and_decide_calls_answer', async () => {
    vi.mocked(api.teamQuestions).mockResolvedValue([
      question('q1', { prompt: { ...question('q1').prompt, text: 'Raise the price\nby 5%' } }),
      question('q2', { source: 'native-permission', seat: B, prompt: { ...question('q2').prompt, tool: 'Bash' } }),
      question('q3', { source: 'request-input' }),
      question('q4', { state: 'answered' }),
    ]);
    vi.mocked(api.answer).mockResolvedValue(question('q1', { state: 'answered' }));
    const { result } = await open(snapshot());
    await waitFor(() => expect(result.current.approvals).toHaveLength(3));
    expect(result.current.approvals).toEqual([
      { id: 'q1', agent: A, title: 'Raise the price', detail: 'Raise the price\nby 5%', kind: 'change' },
      { id: 'q2', agent: B, title: 'Use Bash: Prompt q2', detail: 'Prompt q2', kind: 'question' },
      { id: 'q3', agent: A, title: 'Prompt q3', kind: 'answer' },
    ]);
    // Every open question waits in the feed, so the question-waiting rule can time it.
    expect(result.current.feed.filter((f) => f.alarm === 'question-waiting').map((f) => [f.text, f.alarmKey])).toEqual([
      ['Builder asks you to approve a change', 'q1'], ['b asks to use Bash', 'q2'], ['Builder asked you a question', 'q3'],
    ]);
    await act(() => result.current.decide('q1', { allow: true }));
    await act(() => result.current.decide('q2', { allow: false, reason: 'too risky' }));
    expect(vi.mocked(api.answer).mock.calls.map((c) => c.slice(0, 2))).toEqual([['q1', { kind: 'allow' }], ['q2', { kind: 'deny', reason: 'too risky' }]]);
    // A question that needs a written answer is answered in Needs you, never here.
    await expect(result.current.decide('q3', { allow: true })).rejects.toThrow('Answer this in Needs you');
    expect(api.answer).toHaveBeenCalledTimes(2);
  });

  it('the same decision pressed twice is sent once; another decision is a new request', async () => {
    vi.mocked(api.teamQuestions).mockResolvedValue([question('q1')]);
    const releases: (() => void)[] = [];
    vi.mocked(api.answer).mockImplementation(() => new Promise((r) => { releases.push(() => r(question('q1', { state: 'answered' }))); }));
    const { result } = await open(snapshot());
    await waitFor(() => expect(result.current.approvals).toHaveLength(1));
    let both: Promise<unknown> = Promise.resolve();
    act(() => { both = Promise.all([result.current.decide('q1', { allow: true }), result.current.decide('q1', { allow: true })]); });
    expect(api.answer).toHaveBeenCalledTimes(1);
    let deny: Promise<unknown> = Promise.resolve();
    act(() => { deny = result.current.decide('q1', { allow: false, reason: 'no' }); });
    expect(api.answer).toHaveBeenCalledTimes(2);
    await act(async () => { releases.forEach((r) => r()); await Promise.all([both, deny]); });
  });

  it('emergency_stop_unavailable_when_nothing_running', async () => {
    const stopped = snapshot({ seats: [seatView('a', { lifecycle: 'stopped' }), seatView('b', { lifecycle: 'stopped' })] });
    const { result } = await open(stopped);
    const s = result.current;
    expect(s.running).toBe(false);
    expect(s.team.emergencyStop.can).toEqual({ ok: false, why: 'Nothing is running' });
    expect(s.team.pauseAll.can).toEqual({ ok: false, why: 'Nothing is running' });
    expect(s.team.resumeAll.can).toEqual({ ok: false, why: 'Nothing is paused' });
    expect(s.team.start.can).toEqual({ ok: true });
    expect(s.team.stop.can).toEqual({ ok: false, why: 'Nothing is running' });
    // Without the project bar, Start says where to start (read when pressed).
    await expect(s.team.start.run()).resolves.toEqual({ text: 'Start the team from its project page.', skipped: [], refused: true });
    await expect(s.team.emergencyStop.run()).resolves.toEqual({ text: 'Nothing is running', skipped: [], refused: true });
    expect(api.stopTeamNow).not.toHaveBeenCalled();

    // Running: pressed twice, it stops once.
    vi.mocked(api.stopTeamNow).mockResolvedValue({ stopped: 2 });
    act(() => stream().emit('snapshot', snapshot()));
    expect(result.current.team.emergencyStop.can).toEqual({ ok: true, note: 'Stops 2 agents' });
    const stop = result.current.team.emergencyStop;
    let reports: unknown[] = [];
    await act(async () => { reports = await Promise.all([stop.run(), stop.run()]); });
    expect(api.stopTeamNow).toHaveBeenCalledTimes(1);
    expect(reports[0]).toEqual({ text: 'Stopping 2 agents now, with every agent they started.', skipped: [] });
    // The graceful Stop team… is the project bar's; without one it says where to stop, as a refusal.
    expect(result.current.team.stop.can).toEqual({ ok: true });
    await expect(result.current.team.stop.run()).resolves.toEqual({ text: 'Stop the team from its project page.', skipped: [], refused: true });
  });

  it('alarm ids stay the same with the event in or out of the list, and across a disconnect', async () => {
    const failedB = snapshot({ seats: [seatView('a'), seatView('b', { lifecycle: 'failed', lifecycle_reason: 'crashed', tenure: 3 })] });
    vi.mocked(api.teamQuestions).mockResolvedValue([question('q3', { source: 'request-input' })]);
    const LATER = 10 * 60_000;
    const ids = (r: { current: { feed: Parameters<typeof alarmsFrom>[0]; agents: Parameters<typeof alarmsFrom>[1] } }) =>
      alarmsFrom(r.current.feed, r.current.agents, DEFAULT_RULES, LATER).map((a) => a.id).sort();
    const want = [`failed:${B}:3`, 'question-waiting:q3'];
    const withEvents: Record<string, EventRecord[]> = {
      [A]: [ev(1, { type: 'question-raised', question_id: 'q3', source: 'request-input', prompt: question('q3').prompt, turn_id: null })],
      [B]: [ev(2, { type: 'tenure-started', tenure: 3, cause: 'fresh', driver: 'stub', native_session_id: null, caps: seatView('b').caps! }, B),
        ev(3, { type: 'seat-lifecycle-changed', lifecycle: 'failed', reason: 'crashed' }, B)],
    };
    const first = await open(failedB, withEvents);
    await waitFor(() => expect(first.result.current.feed.filter((f) => f.alarm)).toHaveLength(2));
    expect(ids(first.result)).toEqual(want);
    first.unmount();

    const second = await open(failedB, {});
    await waitFor(() => expect(second.result.current.feed.some((f) => f.alarmKey === 'q3')).toBe(true));
    expect(ids(second.result)).toEqual(want);
    act(() => stream().fail());
    await waitFor(() => expect(second.result.current.agents[1].status.word).toBe('failed (last known)'));
    expect(ids(second.result)).toEqual(want);
  });
});
