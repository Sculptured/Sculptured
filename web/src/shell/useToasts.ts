import { useCallback, useEffect, useRef, useState } from 'react';
import type { TeamRow } from './model';
import type { Toast } from './Toasts';

/**
 * Toasts when a background agent becomes "needs you" or "done". First sight never toasts, the
 * agent you are looking at never toasts, and a turn that ends on the board you watch is already
 * in front of you.
 */
export function useToasts(rows: TeamRow[], openAgent: string | undefined, boardTeam: string | undefined) {
  const [list, setList] = useState<Toast[]>([]);
  const id = useRef(0);
  const prev = useRef(new Map<string, string>());
  useEffect(() => {
    const fresh: Toast[] = [];
    for (const t of rows) {
      if (!t.loaded) continue;
      for (const a of t.agents) {
        const before = prev.current.get(a.id);
        prev.current.set(a.id, a.state.key);
        if (before === undefined || before === a.state.key || a.id === openAgent) continue;
        if (a.state.key === 'done' && a.rig === boardTeam) continue;
        // "done" means a turn just ended; idle becoming done is only history arriving.
        if (a.state.key === 'needs-you' || (a.state.key === 'done' && before === 'working'))
          fresh.push({ id: ++id.current, agent: a.id, rig: a.rig, name: a.name, kind: a.state.key, team: t.title });
      }
    }
    if (fresh.length > 0) setList((cur) => [...cur.filter((x) => !fresh.some((f) => f.agent === x.agent)), ...fresh].slice(-5));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only a change of rows can make a toast
  }, [rows, openAgent]);
  const dismiss = useCallback((n: number) => setList((cur) => cur.filter((t) => t.id !== n)), []);
  return { list, dismiss };
}
