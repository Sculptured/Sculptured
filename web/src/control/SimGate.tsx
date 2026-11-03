import type { FC, ReactNode } from 'react';
import { ArenaProblem } from '../arena/gate';
import type { PaneProps } from '../workspace/registry';
import { useControl } from './context';

/**
 * A simulated run's pane before its branch is on screen: "Loading the run…", or why it can't be
 * read in plain words ("Offline: Arena is not running" when Arena is not answering; the header
 * says how to start it). A deployment's panes, and a loaded run's, show their body.
 */
export function SimGate({ children }: { children: ReactNode }) {
  const { source } = useControl();
  const a = source.arena;
  if (source.kind !== 'simulated' || !a || a.branch) return <>{children}</>;
  if (a.error) return <div className="ctl-empty"><ArenaProblem error={a.error} /></div>;
  return <p className="ctl-empty" role="status">Loading the run…</p>;
}

/** `Body` behind the gate. */
export function gated(Body: FC<PaneProps>): FC<PaneProps> {
  return function Gated(props: PaneProps) {
    return <SimGate><Body {...props} /></SimGate>;
  };
}
