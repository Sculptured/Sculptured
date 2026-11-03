import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeEventSource } from '../test-fixtures';
import { arenaFake, branchB1, ev, resetArena } from './simHarness';
import { DECIDED_BY_AGENTS } from './simulatedFeed';
import { ARENA_CANT_CONTROL, NO_MESSAGES, START_IN_ARENA, useSimulatedSource } from './simulatedSource';
import { ActionError } from './source';

beforeEach(resetArena);
afterEach(() => vi.unstubAllGlobals());

const wrapper = ({ children }: { children: ReactNode }) => <MemoryRouter initialEntries={['/arena/runs/r1/b1']}>{children}</MemoryRouter>;

async function source() {
  const hook = renderHook(() => useSimulatedSource('r1', 'b1'), { wrapper });
  await waitFor(() => expect(hook.result.current.arena?.branch).toBeDefined());
  await waitFor(() => expect(hook.result.current.arena?.branches.length).toBe(1));
  return hook;
}

describe('useSimulatedSource', () => {
  it('simulated_source_maps_events', async () => {
    const { result } = await source();
    const s = result.current;
    expect(s.kind).toBe('simulated');
    expect(s.title).toBe('Task 27 · Original');
    expect(s.runStatus).toEqual({ glyph: '✓', word: 'passed the check', tone: 'idle' });
    expect(s.running).toBe(false);
    expect(s.endedAt! - s.startedAt!).toBe(20_000);
    // Agents in the team's order; "done" is what each reported (grey), never verified.
    expect(s.agents.map((a) => [a.name, `${a.status.glyph} ${a.status.word}`])).toEqual([['Triage', '✓ done (reported)'], ['Resolver', '✓ done (reported)']]);
    const text = (seq: number) => s.feed.find((i) => i.id === `s${seq}`)?.text;
    expect(text(0)).toBe('The run started (task 27)');
    expect(text(2)).toBe('Triage used get_order_details');
    expect(text(7)).toBe('Resolver used exchange_delivered_order_items and changed 1 row · saved plant, retail');
    expect(text(11)).toBe('The final check passed');
    expect(s.feed.find((i) => i.id === 's7')?.raw).toMatchObject({ seq: 7, kind: 'tool_call' });
    // Approvals come from the world's Sculptured part: c2 was decided, c1 still waits.
    expect(s.approvals).toEqual([expect.objectContaining({ id: 'c1', agent: 'triage', title: 'Refund order #W1', kind: 'change', can: { ok: false, why: DECIDED_BY_AGENTS } })]);
    expect(s.approvals[0].detail).toBe('Action: refund · System: shop · Reason: Boots arrived damaged');
    // Tokens as each agent reported at its end; no dollar figure, since Resolver gave none.
    expect(s.metrics.tokens).toBe(300);
    expect(s.metrics.costUsd).toBeUndefined();
    const tl = s.timeline!;
    expect(tl.steps.map((x) => x.seq)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    expect(tl.saves).toEqual([3, 7, 8, 9]);
    expect(tl.cursor).toBe(11);
    expect(s.world?.save).toBe(9);
    expect(s.arena?.parts?.map((p) => p.name)).toEqual(['retail', 'plant', 'desk']);
    await waitFor(() => expect(result.current.arena?.reason).toBe('Exchange the hiking boots for a larger size.'));
    // Rows through the source: any step's, in a given part.
    await expect(result.current.world!.read(7, ['orders/#W1'], 'retail')).resolves.toEqual({ 'orders/#W1': { status: 'exchange requested' } });
    expect(arenaFake.calls.at(-1)?.url).toBe('/api/arena/branches/b1/rows?seq=7&keys=orders%2F%23W1&part=retail');
  });

  it('has no approvals without a Sculptured part in its world', async () => {
    const b = branchB1();
    b.events[0] = ev(0, 0, null, 'run_start', { task_id: '27', world: [{ part: 'retail', integration: 'mongo' }] });
    arenaFake.branches.b1 = b;
    const { result } = await source();
    expect(result.current.approvals).toEqual([]);
  });

  it('controls_unavailable_with_reason', async () => {
    const { result } = await source();
    const s = result.current;
    for (const k of ['pauseAll', 'resumeAll', 'stop', 'emergencyStop'] as const) expect(s.team[k].can).toEqual({ ok: false, why: ARENA_CANT_CONTROL });
    expect(s.team.start.can).toEqual({ ok: false, why: START_IN_ARENA });
    // Pressed anyway: a refusal in words, never a success, and nothing is sent.
    await expect(s.team.emergencyStop.run()).resolves.toEqual({ text: ARENA_CANT_CONTROL, skipped: [], refused: true });
    for (const a of ['pause', 'resume', 'stop'] as const) expect(s.agentControl('triage', a).can).toEqual({ ok: false, why: ARENA_CANT_CONTROL });
    expect(s.message('triage', 'hi').can).toEqual({ ok: false, why: NO_MESSAGES });
    await expect(s.decide('c1', { allow: true })).rejects.toBeInstanceOf(ActionError);
    expect(arenaFake.calls.filter((c) => c.method !== 'GET')).toEqual([]);
  });

  it('a failed agent raises one alarm, by status and by event alike', async () => {
    const b = branchB1();
    b.events[4] = ev(5, 7, 'triage', 'agent_end', { ok: false, error: 'exit 1', tokens: null, cost_usd: null, cost_source: null, seconds: 7 });
    arenaFake.branches.b1 = b;
    const { result } = await source();
    const triage = result.current.agents[0];
    expect(triage.status.word).toBe('failed');
    const item = result.current.feed.find((i) => i.id === 's5')!;
    expect(item.text).toBe('Triage stopped with an error');
    expect(`${item.alarm}:${item.alarmKey}`).toBe(`${triage.alarm!.rule}:${triage.alarm!.key}`);
  });

  it('follows a running branch: new steps join, and the cursor stays on the latest', async () => {
    const b = branchB1();
    b.status = 'running';
    b.ended_at = null;
    b.events = b.events.slice(0, 8);
    arenaFake.branches.b1 = b;
    const { result } = await source();
    expect(result.current.running).toBe(true);
    expect(result.current.agents[1].status).toMatchObject({ word: 'working', tone: 'ok' });
    expect(result.current.timeline!.cursor).toBe(7);
    const stream = FakeEventSource.instances.find((e) => e.url.endsWith('/branches/b1/stream'))!;
    act(() => stream.emit('event', ev(8, 16, 'resolver', 'model_call', {})));
    expect(result.current.timeline!.cursor).toBe(8);
    expect(result.current.agents[1].activity).toBe('calling its model');
  });

  it('simulated_arena_down_is_plain', async () => {
    arenaFake.down = true;
    const hook = renderHook(() => useSimulatedSource('r1', 'b1'), { wrapper });
    await waitFor(() => expect(hook.result.current.arena?.error).toBeDefined());
    expect(hook.result.current.arena?.branch).toBeUndefined();
    expect((hook.result.current.arena?.error as { code?: string }).code).toBe('arena_unreachable');
    // Arena comes back: the branch is read again on the next health check.
    arenaFake.down = false;
    act(() => { window.dispatchEvent(new Event('sculptured:tests-health')); });
    await waitFor(() => expect(hook.result.current.arena?.branch?.id).toBe('b1'));
    expect(hook.result.current.arena?.error).toBeUndefined();
  });
});
