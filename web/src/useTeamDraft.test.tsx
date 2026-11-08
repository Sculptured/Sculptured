import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from './api';
import { fitGroups, groupsFit, moveAgent, removeElement, renameAgent } from './draft/edits';
import type { DraftGroup } from './generated/DraftGroup';
import type { TeamDraft } from './generated/TeamDraft';
import type { TeamRecord } from './generated/TeamRecord';
import { draftAgent, teamDraft, teamRecord } from './test-fixtures';
import { useTeamDraft } from './useTeamDraft';

vi.mock('./api', async (orig) => {
  const m = await orig<typeof import('./api')>();
  return { ...m, api: { ...m.api, getTeam: vi.fn(), saveTeam: vi.fn() } };
});

function serverSaves(rec: TeamRecord, over: Partial<TeamRecord> = {}) {
  vi.mocked(api.saveTeam).mockImplementation(async (_id: string, revision: number, draft: TeamDraft) => ({
    ...rec,
    ...over,
    draft: { ...draft, revision: revision + 1 },
  }));
}

async function loaded(rec: TeamRecord = teamRecord()) {
  vi.mocked(api.getTeam).mockResolvedValue(rec);
  serverSaves(rec);
  const hook = renderHook(() => useTeamDraft('t', 5));
  await waitFor(() => expect(hook.result.current.draft).not.toBeNull());
  return hook;
}

beforeEach(() => {
  vi.mocked(api.getTeam).mockReset();
  vi.mocked(api.saveTeam).mockReset();
});

describe('useTeamDraft', () => {
  it('shows an unfit team fitted, saves nothing until the first edit, then saves it fitted', async () => {
    const groups: DraftGroup[] = [{ id: 'g1', label: 'g1', x: 16, y: 0, w: 248, h: 260 }, { id: 'g2', label: 'g2', x: 300, y: 0, w: 248, h: 260 }];
    const d = teamDraft({
      groups,
      agents: [draftAgent('builder', { x: 40, y: 72, group_id: 'g1' }), draftAgent('reviewer', { x: 324, y: 72, group_id: 'g2' })],
    });
    expect(groupsFit(d)).toBe(false);
    const { result, unmount } = await loaded(teamRecord({ draft: d }));
    expect(groupsFit(result.current.draft!)).toBe(true);
    expect(result.current.canUndo).toBe(false);
    await new Promise((r) => setTimeout(r, 30));
    unmount();
    expect(api.saveTeam).not.toHaveBeenCalled();
    const again = await loaded(teamRecord({ draft: d }));
    act(() => void again.result.current.edit((x) => renameAgent(x, 'builder', 'B')));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(1));
    const [, revision, saved] = vi.mocked(api.saveTeam).mock.calls[0];
    expect(revision).toBe(d.revision);
    expect(groupsFit(saved)).toBe(true);
    expect(saved.groups).toEqual(fitGroups(d).groups);

    // A reload of an unfit record is fitted too, and saves nothing.
    vi.mocked(api.saveTeam).mockClear();
    const third = await loaded(teamRecord({ draft: d }));
    await act(() => third.result.current.reload());
    expect(groupsFit(third.result.current.draft!)).toBe(true);
    expect(third.result.current.canUndo).toBe(false);
    await new Promise((r) => setTimeout(r, 30));
    third.unmount();
    expect(api.saveTeam).not.toHaveBeenCalled();
  });

  it('re-checks a missing computer until it connects, then stops asking', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const computer = { element: { kind: 'team', id: 't' }, severity: 'error', code: 'computer-not-connected', message: 'Connect your computer', detail: null, spec_path: null } as TeamRecord['issues'][number];
      const { result } = await loaded(teamRecord({ issues: [computer] }));
      expect(result.current.issues.map((i) => i.code)).toEqual(['computer-not-connected']);
      vi.mocked(api.getTeam).mockResolvedValue(teamRecord({ issues: [] }));
      const before = vi.mocked(api.getTeam).mock.calls.length;
      await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
      await waitFor(() => expect(result.current.issues).toEqual([]));
      const after = vi.mocked(api.getTeam).mock.calls.length;
      expect(after).toBeGreaterThan(before);
      await act(async () => { await vi.advanceTimersByTimeAsync(20_000); });
      expect(vi.mocked(api.getTeam).mock.calls.length).toBe(after);
    } finally {
      vi.useRealTimers();
    }
  });

  it('autosaves after the last edit with the revision and a fresh request id each time', async () => {
    const { result } = await loaded();
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'B'), 'name'));
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Bo'), 'name'));
    expect(result.current.saveState).toBe('unsaved');
    await waitFor(() => expect(result.current.saveState).toBe('saved'));
    expect(api.saveTeam).toHaveBeenCalledTimes(1);
    const [id, revision, draft, reqA] = vi.mocked(api.saveTeam).mock.calls[0];
    expect([id, revision, draft.agents[0].name]).toEqual(['t', 1, 'Bo']);
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Bob'), 'other'));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(2));
    const [, revision2, , reqB] = vi.mocked(api.saveTeam).mock.calls[1];
    expect(revision2).toBe(2);
    expect(reqB).not.toBe(reqA);
  });

  it('undo and redo restore the draft and save it', async () => {
    const { result } = await loaded();
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Changed')));
    act(() => result.current.undo());
    expect(result.current.draft!.agents[0].name).toBe('Builder');
    expect(result.current.canRedo).toBe(true);
    act(() => result.current.redo());
    expect(result.current.draft!.agents[0].name).toBe('Changed');
  });

  it('one undo reverts an edit together with the group it pushed out of the way', async () => {
    const grp = (id: string, x: number): DraftGroup => ({ id, label: id, x, y: 0, w: 248, h: 276 });
    const d = teamDraft({
      groups: [grp('team', 16), grp('g2', 280)],
      agents: [draftAgent('builder', { x: 40, y: 48, group_id: 'team' }), draftAgent('reviewer', { x: 304, y: 48, group_id: 'g2' })],
    });
    const { result } = await loaded(teamRecord({ draft: d }));
    act(() => void result.current.edit((x) => moveAgent(x, 'builder', { x: 96, y: 48 })));
    const pushed = result.current.draft!.groups.find((g) => g.id === 'g2')!;
    expect(pushed.x).toBeGreaterThan(280);
    act(() => result.current.undo());
    expect(result.current.draft).toEqual(d);
    expect(result.current.canUndo).toBe(false);
  });

  it('whole-group deletion is one undo step and restores its agents and arrows', async () => {
    const grp = (id: string, x: number): DraftGroup => ({ id, label: id, x, y: 0, w: 248, h: 276 });
    const d = teamDraft({
      groups: [grp('team', 16), grp('g2', 300)],
      agents: [draftAgent('builder', { x: 40, y: 48, group_id: 'team' }), draftAgent('reviewer', { x: 324, y: 48, group_id: 'g2' })],
    });
    const { result } = await loaded(teamRecord({ draft: d }));
    let reason: string | null = null;
    act(() => {
      reason = result.current.edit((x) => removeElement(x, { kind: 'group', id: 'g2' }));
    });
    expect(reason).toBeNull();
    expect(result.current.canUndo).toBe(true);
    expect(result.current.draft?.agents.map(a=>a.id)).toEqual(['builder']);
    expect(result.current.draft?.connections).toEqual([]);
    act(()=>result.current.undo());
    expect(result.current.draft).toEqual(d);

  });

  it('a conflict stops autosave, keeps the local edits and offers reload', async () => {
    const { result } = await loaded();
    vi.mocked(api.saveTeam).mockRejectedValue(new ApiError(409, 'draft_conflict', 'This team was changed somewhere else (it is now at version 4).'));
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Mine')));
    await waitFor(() => expect(result.current.saveState).toBe('conflict'));
    expect(result.current.saveError).toMatch(/changed somewhere else/);
    expect(result.current.draft!.agents[0].name).toBe('Mine');
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Mine 2')));
    await new Promise((r) => setTimeout(r, 30));
    expect(api.saveTeam).toHaveBeenCalledTimes(1);
    vi.mocked(api.getTeam).mockResolvedValue(teamRecord({ draft: teamDraft({ revision: 4, name: 'Theirs' }) }));
    await act(() => result.current.reload());
    expect(result.current.saveState).toBe('saved');
    expect(result.current.draft!.name).toBe('Theirs');
  });

  it('never takes the running state from a save response', async () => {
    const rec = teamRecord();
    vi.mocked(api.getTeam).mockResolvedValue(rec);
    serverSaves(rec, { live: true });
    const { result } = renderHook(() => useTeamDraft('t', 5));
    await waitFor(() => expect(result.current.draft).not.toBeNull());
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'X')));
    await waitFor(() => expect(result.current.saveState).toBe('saved'));
    expect(result.current.serverLive).toBe(false);
    vi.mocked(api.getTeam).mockResolvedValue({ ...rec, live: true });
    await act(async () => void (await result.current.refresh()));
    expect(result.current.serverLive).toBe(true);
  });

  it('flush saves now and reports the saved revision', async () => {
    const { result } = await loaded();
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Now')));
    let out: { ok: boolean; revision: number } | undefined;
    await act(async () => {
      out = await result.current.flush();
    });
    expect(out).toEqual({ ok: true, revision: 2 });
  });
});

describe('useTeamDraft: leaving with unsaved edits', () => {
  async function slow(rec: TeamRecord = teamRecord()) {
    vi.mocked(api.getTeam).mockResolvedValue(rec);
    serverSaves(rec);
    // The autosave would wait 10 s; leaving must not wait for it.
    const hook = renderHook(() => useTeamDraft('t', 10_000));
    await waitFor(() => expect(hook.result.current.draft).not.toBeNull());
    return hook;
  }

  it('an edit made just before leaving is saved when the board closes', async () => {
    const { result, unmount } = await slow();
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Last word'), 'name'));
    unmount();
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(1));
    const [id, revision, draft] = vi.mocked(api.saveTeam).mock.calls[0];
    expect([id, revision, draft.agents[0].name]).toEqual(['t', 1, 'Last word']);
  });

  it('a newer edit waiting behind a save in flight is still sent after the board closes', async () => {
    const rec = teamRecord();
    vi.mocked(api.getTeam).mockResolvedValue(rec);
    serverSaves(rec);
    const { result, unmount } = renderHook(() => useTeamDraft('t', 5));
    await waitFor(() => expect(result.current.draft).not.toBeNull());
    let release!: () => void;
    vi.mocked(api.saveTeam).mockImplementationOnce(
      (_id: string, revision: number, draft: TeamDraft) =>
        new Promise((resolve) => {
          release = () => resolve({ ...rec, draft: { ...draft, revision: revision + 1 } });
        }),
    );
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'First'), 'a'));
    // The autosave starts and is still waiting for the server.
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(1));
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Second'), 'b'));
    unmount();
    release();
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(2));
    const [, revision, draft] = vi.mocked(api.saveTeam).mock.calls[1];
    expect([revision, draft.agents[0].name]).toEqual([2, 'Second']);
  });

  it('closes without a request when nothing is unsaved', async () => {
    const { unmount } = await slow();
    unmount();
    await Promise.resolve();
    expect(api.saveTeam).not.toHaveBeenCalled();
  });

  it('when the page goes away it sends the unsaved draft with keepalive; if the page stays, that answer is the save', async () => {
    const { result } = await slow();
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Bye'), 'name'));
    act(() => void window.dispatchEvent(new Event('pagehide')));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(1));
    const [id, revision, draft, requestId, opts] = vi.mocked(api.saveTeam).mock.calls[0];
    expect([id, revision, draft.agents[0].name, opts]).toEqual(['t', 1, 'Bye', { keepalive: true }]);
    // The page stayed (back/forward cache): the answer counts as the save.
    await waitFor(() => expect(result.current.saveState).toBe('saved'));
    expect(typeof requestId).toBe('string');
    // Nothing is left to send.
    await act(async () => void (await result.current.flush()));
    expect(api.saveTeam).toHaveBeenCalledTimes(1);
  });

  it('if the keepalive save fails and the page stays, autosave retries it with the same request id', async () => {
    const rec = teamRecord();
    vi.mocked(api.getTeam).mockResolvedValue(rec);
    serverSaves(rec);
    vi.mocked(api.saveTeam).mockRejectedValueOnce(new TypeError('Failed to fetch'));
    const { result } = renderHook(() => useTeamDraft('t', 5));
    await waitFor(() => expect(result.current.draft).not.toBeNull());
    act(() => void result.current.edit((d) => renameAgent(d, 'builder', 'Bye'), 'name'));
    act(() => void window.dispatchEvent(new Event('pagehide')));
    await waitFor(() => expect(api.saveTeam).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.saveState).toBe('saved'));
    const [first, second] = vi.mocked(api.saveTeam).mock.calls;
    expect(first[4]).toEqual({ keepalive: true });
    expect(second[4]).toBeUndefined();
    expect(second[3]).toBe(first[3]);
    expect([second[1], second[2].agents[0].name]).toEqual([1, 'Bye']);
  });

  it('sends nothing on pagehide when everything is saved', async () => {
    await slow();
    act(() => void window.dispatchEvent(new Event('pagehide')));
    await Promise.resolve();
    expect(api.saveTeam).not.toHaveBeenCalled();
  });
});
