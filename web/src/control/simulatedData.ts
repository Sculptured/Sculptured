import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import { arena, follow } from '../tests/api';
import { useArenaHealth } from '../tests/shared';
import type { Branch, BranchDetail, Event, Health, WorldPartInfo } from '../tests/types';

/**
 * What a simulated source reads from Arena: the branch with its events (followed while it runs),
 * its run's branches, what each part of the world says about itself, and Arena's health. A
 * response only lands while it is still for the branch on screen. When Arena comes back after not
 * answering, everything is read again.
 */
export type ArenaRunData = {
  detail: BranchDetail | null;
  /** Why the branch could not be read (the last attempt); null once it is. */
  error: unknown;
  branches: Branch[];
  parts: WorldPartInfo[] | null;
  health: Health | null | undefined;
  healthError: unknown;
};

type Shown = { id: string; detail: BranchDetail | null; error: unknown };

function useBranchDetail(branchId: string) {
  const [state, setState] = useState<Shown>({ id: branchId, detail: null, error: null });
  const current = useRef(branchId);
  current.current = branchId;
  const load = useCallback(() => {
    const id = branchId;
    arena.branch(id)
      .then((detail) => { if (current.current === id) setState({ id, detail, error: null }); })
      .catch((error: unknown) => { if (current.current === id) setState((s) => ({ id, detail: s.id === id ? s.detail : null, error })); });
  }, [branchId]);
  useEffect(load, [load]);
  const mine = state.id === branchId;
  return { detail: mine ? state.detail : null, error: mine ? state.error : null, load, setState };
}

/** Each new step of a running branch as it is recorded; its status changing reads it again. */
function useFollow(detail: BranchDetail | null, setState: Dispatch<SetStateAction<Shown>>, reload: () => void) {
  const id = detail?.id;
  const running = detail?.status === 'running';
  useEffect(() => {
    if (!running || !id) return;
    const add = (e: Event) => setState((s) => {
      const d = s.detail;
      if (!d || d.id !== id || d.events.some((x) => x.seq === e.seq)) return s;
      return { ...s, detail: { ...d, events: [...d.events, e] } };
    });
    return follow(id, add, reload);
  }, [running, id, setState, reload]);
}

/** The run's branches (for names, switching and Compare), read again when `tick` changes. */
function useRunBranches(run: string, tick: unknown) {
  const [branches, setBranches] = useState<Branch[]>([]);
  useEffect(() => {
    let alive = true;
    arena.runs().then((rs) => { if (alive) setBranches(rs.find((r) => r.id === run)?.branches ?? []); }).catch(() => {});
    return () => { alive = false; };
  }, [run, tick]);
  return branches;
}

/** What each part's integration says about itself, read once Arena answers. */
function useParts(up: boolean) {
  const [parts, setParts] = useState<WorldPartInfo[] | null>(null);
  useEffect(() => {
    if (!up) return;
    let alive = true;
    arena.world().then((w) => { if (alive) setParts(w.parts); }).catch(() => {});
    return () => { alive = false; };
  }, [up]);
  return parts;
}

/** Calls `reload` when Arena answers again after not answering. */
function useBackOnline(health: Health | null | undefined, reload: () => void) {
  const wasDown = useRef(false);
  useEffect(() => {
    if (health === null) wasDown.current = true;
    else if (health && wasDown.current) { wasDown.current = false; reload(); }
  }, [health, reload]);
}

export function useArenaRun(run: string, branchId: string): ArenaRunData {
  const { health, error: healthError } = useArenaHealth();
  const { detail, error, load, setState } = useBranchDetail(branchId);
  useFollow(detail, setState, load);
  useBackOnline(health, load);
  const branches = useRunBranches(run, `${detail?.status}:${detail?.id}:${health ? 'up' : 'down'}`);
  const parts = useParts(!!health);
  return { detail, error, branches, parts, health, healthError };
}
