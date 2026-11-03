import { useCallback, useRef, useState } from 'react';
import { ApiError, newRequestId } from '../api';
import { errorText } from '../errors';
import { ActionError, type Action, type ActionReport, type Availability } from './source';

type Call = (requestId: string) => Promise<ActionReport | void>;
export type MakeAction = (key: string, can: Availability, busyWords: string, call: Call) => Action;

/**
 * Makes `Action`s that run once at a time. While one runs, its `can` says `busyWords` and `run()`
 * returns the same promise; an action that cannot run resolves with its reason (`refused`) and calls nothing.
 * Each action keeps its request id across a network failure (a retry executes once) and renews it
 * once the server answers. Failures reject with an `ActionError` in plain words. `after` runs when
 * each action settles (to reload what it changed).
 */
export function useActionRunner(after: () => void): MakeAction {
  const [busy, setBusy] = useState<Record<string, string>>({});
  const flights = useRef(new Map<string, Promise<ActionReport | void>>());
  const ids = useRef(new Map<string, string>());
  const start = useCallback((key: string, words: string, call: Call) => {
    const id = ids.current.get(key) ?? newRequestId();
    ids.current.set(key, id);
    setBusy((b) => ({ ...b, [key]: words }));
    const settled = () => {
      flights.current.delete(key);
      setBusy(({ [key]: _done, ...rest }) => rest);
      after();
    };
    const p = call(id).then(
      (r) => { ids.current.delete(key); settled(); return r; },
      (e: unknown) => { if (e instanceof ApiError) ids.current.delete(key); settled(); throw new ActionError(errorText(e)); },
    );
    flights.current.set(key, p);
    return p;
  }, [after]);
  return useCallback((key, can, busyWords, call) => ({
    can: busy[key] ? { ok: false, why: busy[key] } : can,
    run: () => flights.current.get(key) ?? (can.ok ? start(key, busyWords, call) : Promise.resolve({ text: can.why, skipped: [], refused: true as const })),
  }), [busy, start]);
}
