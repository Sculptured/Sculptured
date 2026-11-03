import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { RigSnapshot } from '../generated/RigSnapshot';
import { ShellContext, type RunHandle, type Shell } from '../shell/ShellContext';
import { ev, FakeEventSource, openFake, seatView, snapshot } from '../test-fixtures';
import { ALL_TEAMS_STREAM, useTeams } from '../useTeams';
import { useDeploymentSource } from './deploymentSource';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const fns = ['listRigs', 'latestSeatEvents', 'teamQuestions', 'answer', 'teamUsage', 'teamBudget', 'runUsage', 'agentControl', 'controlAgent', 'pauseTeam', 'resumeTeam', 'stopTeamNow', 'send'] as const;
  return { ...m, api: { ...m.api, ...Object.fromEntries(fns.map((f) => [f, vi.fn()])) } };
});

const NOW = 60_000;
const A = 'dev.a@r';
const B = 'dev.b@r';
const run: { current: RunHandle | null } = { current: null };

function Wrapper({ children }: { children: ReactNode }) {
  const { teams } = useTeams('r', openFake);
  const shell = { teams, names: { r: { [A]: 'Builder' } }, titles: { r: 'My team' }, now: NOW, refs: { run } } as unknown as Shell;
  return <ShellContext.Provider value={shell}>{children}</ShellContext.Provider>;
}

const stream = () => FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM).at(-1)!;
async function open(snap: RigSnapshot, history: Record<string, EventRecord[]> = {}) {
  vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) => history[id] ?? []);
  const hook = renderHook(() => useDeploymentSource('r'), { wrapper: Wrapper });
  act(() => stream().emit('snapshot', snap));
  act(() => stream().emit('ready', {}));
  await act(async () => {});
  return hook;
}

function question(id: string, over: Partial<QuestionView> = {}): QuestionView {
  return {
    id, seat: A, rig: 'r', source: 'approval', state: 'open', opened_at_ms: 5, deadline_ms: null, answer: null, answered_at_ms: null, late_answer: null,
    decision: null, withdrawn_reason: null, acknowledged: false, consequence: [], expired_seq: null, unconfirmed: null,
    prompt: { header: null, text: `Prompt ${id}`, options: [], allow_other: false, tool: null, tool_input: null }, ...over,
  };
}

beforeEach(() => {
  FakeEventSource.reset();
  vi.clearAllMocks();
  run.current = null;
  vi.mocked(api.listRigs).mockResolvedValue([]);
  vi.mocked(api.teamQuestions).mockResolvedValue([]);
  vi.mocked(api.teamUsage).mockResolvedValue({ agents: [{ seat: A, points: [{ at_ms: 1, input: 100, output: 50, cache_read: 0 }], reports: 1, context: null }] });
  vi.mocked(api.teamBudget).mockResolvedValue({ output_limit: null, agents: [], waiting: [] });
  vi.mocked(api.runUsage).mockResolvedValue([ev(9, { type: 'usage-reported', turn_id: null, input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost: { usd: 0.5, source: 'claude-code' } })]);
  vi.mocked(api.agentControl).mockResolvedValue({ mode: null });
});

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
      { id: B, name: 'b', status: { glyph: '✕', word: 'failed', tone: 'bad' }, activity: 'failed' },
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
    await waitFor(() => expect(result.current.approvals).toHaveLength(2));
    expect(result.current.approvals).toEqual([
      { id: 'q1', agent: A, title: 'Raise the price', detail: 'Raise the price\nby 5%', kind: 'change' },
      { id: 'q2', agent: B, title: 'Prompt q2', kind: 'question' },
    ]);
    // Every open question waits in the feed, so the question-waiting rule can time it.
    expect(result.current.feed.filter((f) => f.alarm === 'question-waiting').map((f) => f.text)).toEqual([
      'Builder asks you to approve a change', 'b asks to use Bash', 'Builder asked you a question',
    ]);
    await act(() => result.current.decide('q1', { allow: true }));
    await act(() => result.current.decide('q2', { allow: false, reason: 'too risky' }));
    expect(vi.mocked(api.answer).mock.calls.map((c) => c.slice(0, 2))).toEqual([['q1', { kind: 'allow' }], ['q2', { kind: 'deny', reason: 'too risky' }]]);
  });

  it('emergency_stop_unavailable_when_nothing_running', async () => {
    const stopped = snapshot({ seats: [seatView('a', { lifecycle: 'stopped' }), seatView('b', { lifecycle: 'stopped' })] });
    const { result } = await open(stopped);
    const s = result.current;
    expect(s.running).toBe(false);
    expect(s.team.emergencyStop.can).toEqual({ ok: false, why: 'Nothing is running' });
    expect(s.team.pauseAll.can).toEqual({ ok: false, why: 'Nothing is running' });
    expect(s.team.resumeAll.can).toEqual({ ok: false, why: 'Nothing is paused' });
    expect(s.team.start.can).toEqual({ ok: false, why: 'Start the team from its project page.' });
    await expect(s.team.emergencyStop.run()).resolves.toEqual({ text: 'Nothing is running', skipped: [] });
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
  });

  it('pause and resume say why they cannot run, and name the agents a pause left alone', async () => {
    vi.mocked(api.agentControl).mockImplementation(async (id: string) => ({ mode: id === B ? 'paused' : null }));
    const { result } = await open(snapshot());
    await waitFor(() => expect(result.current.agentControl(B, 'pause').can).toEqual({ ok: false, why: 'Already paused.' }));
    expect(result.current.agents[1].status).toEqual({ glyph: '‖', word: 'paused', tone: 'idle' });
    expect(result.current.agentControl(B, 'resume').can).toEqual({ ok: true });
    expect(result.current.agentControl(A, 'resume').can).toEqual({ ok: false, why: 'Not paused, so there is nothing to resume.' });
    expect(result.current.agentControl(A, 'stop').can).toEqual({ ok: true });
    expect(result.current.message(A, '  ').can).toEqual({ ok: false, why: 'Type a message to send.' });
    const answer = { affected: [A], skipped: [{ seat: B, code: 'already_paused' as const, reason: 'server words' }], notes: [{ seat: A, reason: 'A note' }] };
    vi.mocked(api.pauseTeam).mockResolvedValue(answer as Awaited<ReturnType<typeof api.pauseTeam>>);
    let report: unknown;
    await act(async () => { report = await result.current.team.pauseAll.run(); });
    expect(report).toEqual({
      text: 'Paused 1 agent; 1 agent left as they were.',
      skipped: [{ agent: B, name: 'b', reason: 'Already paused.' }],
      notes: [{ agent: A, name: 'Builder', text: 'A note' }],
    });
    vi.mocked(api.send).mockResolvedValue({ message_id: 'm1' });
    await act(async () => { await result.current.message(A, 'Check the tests').run(); });
    expect(vi.mocked(api.send).mock.calls[0].slice(0, 2)).toEqual([A, { text: 'Check the tests', intent: 'instruct', interrupt: false }]);
    vi.mocked(api.controlAgent).mockResolvedValue({ mode: 'hard' });
    await act(async () => { await result.current.agentControl(A, 'stop').run(); });
    expect(vi.mocked(api.controlAgent).mock.calls[0].slice(0, 2)).toEqual([A, 'hard']);
  });
});
