import { useParams } from 'react-router-dom';
import { ARENA_PATH, runPanePath } from '../arena/paths';
import { PopOutPane } from '../workspace/PopOutPane';
import { ControlWorkspace, useControlCtx } from './ControlPage';
import { controlRegistry } from './registry';
import { SimulatedBar } from './SimulatedBar';
import { useSimulatedSource } from './simulatedSource';
import './control.css';
import './simulated.css';
import '../arena/arena.css';
import '../tests/tests.css';

/**
 * Every simulated run shares one workspace page (`arena-run`) for its layouts: a run is short-lived
 * and there are many, so an operator's Replay layout carries from run to run. Each branch has its
 * own selection channel, so two branches open side by side never move each other's cursor, and
 * panes popped out for one branch come back when the workspace moves to another.
 */
export const ARENA_RUN_PAGE = 'arena-run';
export const arenaRunChannel = (branch: string) => `${ARENA_RUN_PAGE}.${branch}`;
/** Alarms (their acknowledgements and rules) are kept per branch. */
export const arenaAlarms = (branch: string) => `arena.${branch}`;
const START_FIX = { text: 'Open Arena control', to: ARENA_PATH };

/** `/arena/runs/:run/:branch`: an Arena run's branch in the Control workspace, with its replay. */
export function SimulatedControlPage() {
  const { run = '', branch = '' } = useParams();
  const source = useSimulatedSource(run, branch);
  return (
    <main className="page control-page ctl-sim-page" aria-label="Simulated run">
      <ControlWorkspace team={arenaAlarms(branch)} page={ARENA_RUN_PAGE} channel={arenaRunChannel(branch)} source={source} paneRoute={(i) => runPanePath(run, branch, i)} startFix={START_FIX}
        lead={<SimulatedBar source={source} />} />
    </main>
  );
}

/** `/arena/runs/:run/:branch/pane/:instance`: one popped-out pane in its own window. */
export function SimulatedPanePage() {
  const { run = '', branch = '', instance = '' } = useParams();
  const source = useSimulatedSource(run, branch);
  const ctx = useControlCtx(arenaAlarms(branch), source);
  return (
    <main className="control-popout" aria-label="Simulated run pane">
      <PopOutPane page={ARENA_RUN_PAGE} channel={arenaRunChannel(branch)} registry={controlRegistry} ctx={ctx} instance={instance} />
    </main>
  );
}
