import { useEffect, useState } from 'react';
import { api } from '../api';
import type { ControlMode } from '../generated/ControlMode';
import { usePolled } from '../usePolling';

/** A poll that never comes again (the caller reads the mode). */
const NEVER = 2 ** 31 - 1;

/**
 * An agent's control mode, and a setter for a local change (shown at once). `known`: the mode as the
 * caller already reads it (a Control pane: its team's controls), so the server is asked for nothing;
 * without it, the mode is asked every 2 s. Each answer (or the caller's next reading) replaces it.
 */
export function useControlMode(seat: string, known?: ControlMode | null) {
  const [mode, setMode] = useState<ControlMode | null>(known ?? null);
  const given = known !== undefined;
  const polled = usePolled(() => (given ? Promise.resolve(null) : api.agentControl(seat)), given ? NEVER : 2000, [seat, given]);
  useEffect(() => { if (polled.data) setMode(polled.data.mode); }, [polled.data]);
  useEffect(() => { if (known !== undefined) setMode(known); }, [known]);
  return [mode, setMode] as const;
}
