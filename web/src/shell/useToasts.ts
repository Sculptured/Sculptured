import { useCallback, useEffect, useRef, useState } from 'react';
import type { TeamRow } from './model';
import type { Toast } from './Toasts';

type Prev = Map<string, string>;

/** Whether this change of an agent's state is worth a toast: it needs you, or a turn just ended. */
function worthAToast(a: TeamRow['agents'][number], before: string | undefined, openAgent: string | undefined, boardTeam: string | undefined): boolean {
  if (before === undefined || before === a.state.key || a.id === openAgent) return false;
  if (a.state.key === 'done' && a.rig === boardTeam) return false;
  // "done" means a turn just ended; idle becoming done is only history arriving.
  return a.state.key === 'needs-you' || (a.state.key === 'done' && before === 'working');
}

function freshToasts(rows: TeamRow[], prev: Prev, next: () => number, openAgent: string | undefined, boardTeam: string | undefined): Toast[] {
  const fresh: Toast[] = [];
  for (const t of rows) {
    if (!t.loaded) continue;
    for (const a of t.agents) {
      const before = prev.get(a.id);
      prev.set(a.id, a.state.key);
      if (worthAToast(a, before, openAgent, boardTeam)) fresh.push({ id: next(), agent: a.id, rig: a.rig, name: a.name, kind: a.state.key as Toast['kind'], team: t.title });
    }
  }
  return fresh;
}

/**
 * Toasts when a background agent becomes "needs you" or "done". First sight never toasts, the
 * agent you are looking at never toasts, and a turn that ends on the board you watch is already
 * in front of you.
 */
export function useToasts(rows: TeamRow[], openAgent: string | undefined, boardTeam: string | undefined) {
  const [list, setList] = useState<Toast[]>([]);
  const id = useRef(0);
  const prev = useRef<Prev>(new Map());
  useEffect(() => {
    const fresh = freshToasts(rows, prev.current, () => ++id.current, openAgent, boardTeam);
    if (fresh.length > 0) setList((cur) => [...cur.filter((x) => !fresh.some((f) => f.agent === x.agent)), ...fresh].slice(-5));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only a change of rows can make a toast
  }, [rows, openAgent]);
  const dismiss = useCallback((n: number) => setList((cur) => cur.filter((t) => t.id !== n)), []);
  return { list, dismiss };
}
