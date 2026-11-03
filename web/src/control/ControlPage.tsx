import { useCallback, useMemo, useRef } from 'react';
import { useParams } from 'react-router-dom';
import { useProject } from '../pages/ProjectContext';
import { controlPanePath } from '../paths';
import { PopOutPane } from '../workspace/PopOutPane';
import { WorkspacePage, type WorkspaceHandle } from '../workspace/WorkspacePage';
import type { ControlCtx } from './context';
import { ControlHeader } from './ControlHeader';
import { controlDefaults } from './defaults';
import { useDeploymentSource } from './deploymentSource';
import { controlRegistry } from './registry';
import type { ControlSource } from './source';
import { useAlarms, useNow } from './useAlarms';
import './control.css';

/**
 * A team's Control workspace: its layouts and selection channel are its own (`control.<team>`), so two
 * teams open in two tabs never steer each other's windows.
 */
export const controlPage = (team: string) => `control.${team}`;

const NO_SHOW = () => {};

/** What the panes read: the source, the team's alarms and a way to bring a pane forward. */
export function useControlCtx(team: string, source: ControlSource, show: (type: string) => void = NO_SHOW): ControlCtx {
  const now = useNow(15_000);
  const alarms = useAlarms(team, source, now);
  return { kind: source.kind, team, source, alarms, show, now };
}

type Props = { team: string; source: ControlSource; paneRoute(instance: string): string };

/** The Control workspace over any source (a deployment here; a simulated run in Arena). */
export function ControlWorkspace({ team, source, paneRoute }: Props) {
  const handle = useRef<WorkspaceHandle>(null);
  const show = useCallback((type: string) => handle.current?.show(type), []);
  const ctx = useControlCtx(team, source, show);
  const defaults = useMemo(() => () => controlDefaults(source.kind), [source.kind]);
  return (
    <WorkspacePage page={controlPage(team)} registry={controlRegistry} ctx={ctx} defaults={defaults} paneRoute={paneRoute} handle={handle}
      header={<ControlHeader ctx={ctx} />} />
  );
}

/** `/p/:team/control`: the project's landing page. The default layout puts the live board first. */
export function ControlPage() {
  const p = useProject();
  const source = useDeploymentSource(p.team);
  return (
    <main className="page control-page" aria-label="Control">
      <ControlWorkspace team={p.team} source={source} paneRoute={(i) => controlPanePath(p.team, i)} />
    </main>
  );
}

/** `/p/:team/control/pane/:instance`: one popped-out pane in its own window, without the app's bars. */
export function ControlPanePage() {
  const p = useProject();
  const { instance = '' } = useParams();
  const source = useDeploymentSource(p.team);
  const ctx = useControlCtx(p.team, source);
  return (
    <main className="control-popout" aria-label="Control pane">
      <PopOutPane page={controlPage(p.team)} registry={controlRegistry} ctx={ctx} instance={instance} />
    </main>
  );
}
