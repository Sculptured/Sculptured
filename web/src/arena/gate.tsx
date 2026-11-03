import type { ReactNode } from 'react';
import { ApiError } from '../api';
import { ARENA_DOWN, ARENA_START_COMMAND } from '../errors';
import { ErrorNote } from '../tests/shared';
import { isArenaDown, useArena } from './data';

/** Arena is not answering: say so plainly, with how to start it. What controld saw is evidence. */
export function ArenaDown({ error }: { error?: unknown }) {
  const raw = error instanceof ApiError ? error.raw : undefined;
  return (
    <div className="arena-down" role="status">
      <p className="arena-down-head"><span aria-hidden="true">■</span> {ARENA_DOWN}</p>
      <p>Start it in the arena folder with <code className="mono">{ARENA_START_COMMAND}</code>. This page checks again every 15 seconds.</p>
      {raw && <p className="tst-evidence mono tst-small" data-raw-evidence>Arena said: {raw}</p>}
    </div>
  );
}

/** Any Arena error: "not running" when it is that, otherwise its plain sentence. */
export function ArenaProblem({ error, text }: { error: unknown; text?: string }) {
  if (error == null) return null;
  return isArenaDown(error) ? <ArenaDown error={error} /> : <ErrorNote error={error} text={text} />;
}

/** A pane's body once Arena answers; before that, "asking", or why it isn't answering. */
export function ArenaGate({ children }: { children: ReactNode }) {
  const { health, healthError } = useArena();
  if (health === undefined) return <p className="muted arena-pad">Asking Arena…</p>;
  if (health === null) return <div className="arena-pad"><ArenaProblem error={healthError} /></div>;
  return <>{children}</>;
}
