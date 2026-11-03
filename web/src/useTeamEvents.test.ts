import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import type { EventRecord } from './generated/EventRecord';
import { ev } from './test-fixtures';
import { useTeamEvents } from './useTeamEvents';

vi.mock('./api', async (orig) => {
  const m = await orig<typeof import('./api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn() } };
});

const turn = (seq: number, seat: string) => ev(seq, { type: 'turn-started', turn_id: `t${seq}` }, seat);
const noFeed: Record<string, EventRecord[]> = {};

describe('useTeamEvents', () => {
  beforeEach(() => {
    vi.mocked(api.latestSeatEvents).mockReset();
  });

  it('merges every agent history with the live feed, oldest first, without duplicates', async () => {
    vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) =>
      id === 'team.a@t' ? [turn(1, id), turn(4, id)] : [turn(2, id), turn(3, id)],
    );
    const feed = { 'team.a@t': [turn(4, 'team.a@t'), turn(5, 'team.a@t')] };
    const { result } = renderHook(() => useTeamEvents(['team.a@t', 'team.b@t'], feed, 0, 25));
    await waitFor(() => expect(result.current.events.map((e) => e.seq)).toEqual([1, 2, 3, 4, 5]));
    expect(result.current.error).toBeNull();
    expect(vi.mocked(api.latestSeatEvents).mock.calls).toEqual([
      ['team.a@t', 25],
      ['team.b@t', 25],
    ]);
  });

  it('says from when history is loaded once an agent\'s history was cut at the tail', async () => {
    const at = (e: EventRecord, ms: number) => ({ ...e, at_ms: ms });
    vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) =>
      id === 'team.a@t' ? [at(turn(1, id), 1000), at(turn(4, id), 4000)] : [at(turn(2, id), 2000)],
    );
    const { result } = renderHook(() => useTeamEvents(['team.a@t', 'team.b@t'], noFeed, 0, 2));
    await waitFor(() => expect(result.current.events).toHaveLength(3));
    // a's history filled the tail of 2: older events of a may be missing before 1000.
    expect(result.current.since).toBe(1000);
    const whole = renderHook(() => useTeamEvents(['team.a@t', 'team.b@t'], noFeed, 0, 25));
    await waitFor(() => expect(whole.result.current.events).toHaveLength(3));
    expect(whole.result.current.since).toBeUndefined();
  });

  it('re-fetches on a new snapshot epoch, and fetches nothing for no agents', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([]);
    const { rerender } = renderHook(({ ids, epoch }) => useTeamEvents(ids, noFeed, epoch), {
      initialProps: { ids: [] as string[], epoch: 0 },
    });
    expect(api.latestSeatEvents).not.toHaveBeenCalled();
    rerender({ ids: ['team.a@t'], epoch: 0 });
    await waitFor(() => expect(api.latestSeatEvents).toHaveBeenCalledTimes(1));
    expect(vi.mocked(api.latestSeatEvents).mock.calls[0]).toEqual(['team.a@t', 100]);
    rerender({ ids: ['team.a@t'], epoch: 1 });
    await waitFor(() => expect(api.latestSeatEvents).toHaveBeenCalledTimes(2));
  });

  it('reports a failed history fetch and still shows the live feed', async () => {
    vi.mocked(api.latestSeatEvents).mockRejectedValue(new Error('controld unreachable'));
    const feed = { 'team.a@t': [turn(7, 'team.a@t')] };
    const { result } = renderHook(() => useTeamEvents(['team.a@t'], feed, 0));
    await waitFor(() => expect(result.current.error).toBe('Could not reach Sculptured. Check that it is still running, then try again.'));
    expect(result.current.events.map((e) => e.seq)).toEqual([7]);
  });

  it('reports a rejection that is not an Error in plain words', async () => {
    vi.mocked(api.latestSeatEvents).mockRejectedValue('socket closed');
    const { result } = renderHook(() => useTeamEvents(['team.a@t'], noFeed, 0));
    await waitFor(() => expect(result.current.error).toBe('Could not reach Sculptured. Check that it is still running, then try again.'));
  });

  it('drops history when the agents go away or one is removed', async () => {
    vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) => [turn(id === 'team.a@t' ? 1 : 2, id)]);
    const { result, rerender } = renderHook(({ ids }) => useTeamEvents(ids, noFeed, 0), {
      initialProps: { ids: ['team.a@t', 'team.b@t'] },
    });
    await waitFor(() => expect(result.current.events.map((e) => e.seq)).toEqual([1, 2]));
    rerender({ ids: ['team.a@t'] });
    await waitFor(() => expect(result.current.events.map((e) => e.seq)).toEqual([1]));
    rerender({ ids: [] });
    await waitFor(() => expect(result.current.events).toEqual([]));
  });

  it('prefers the live feed over history for the same seq', async () => {
    vi.mocked(api.latestSeatEvents).mockResolvedValue([ev(3, { type: 'turn-started', turn_id: 'from-history' }, 'team.a@t')]);
    const feed = { 'team.a@t': [ev(3, { type: 'turn-started', turn_id: 'from-feed' }, 'team.a@t')] };
    const { result } = renderHook(() => useTeamEvents(['team.a@t'], feed, 0));
    await waitFor(() => expect(api.latestSeatEvents).toHaveBeenCalled());
    await act(async () => {});
    expect(result.current.events).toHaveLength(1);
    expect(result.current.events[0].body).toEqual({ type: 'turn-started', turn_id: 'from-feed' });
  });

  it('ignores a superseded fetch that resolves late', async () => {
    let releaseOld: (events: EventRecord[]) => void = () => {};
    vi.mocked(api.latestSeatEvents)
      .mockImplementationOnce(() => new Promise((resolve) => (releaseOld = resolve)))
      .mockResolvedValueOnce([turn(2, 'team.a@t')]);
    const { result, rerender } = renderHook(({ epoch }) => useTeamEvents(['team.a@t'], noFeed, epoch), {
      initialProps: { epoch: 0 },
    });
    rerender({ epoch: 1 });
    await waitFor(() => expect(result.current.events.map((e) => e.seq)).toEqual([2]));
    await act(async () => releaseOld([turn(1, 'team.a@t')]));
    expect(result.current.events.map((e) => e.seq)).toEqual([2]);
  });
});
