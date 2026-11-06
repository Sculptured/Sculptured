import { useParams } from 'react-router-dom';
import { StatusTile } from '../../components/StatusTile';
import { splitOf as split, stackOf, type PageLayouts } from '../../workspace/model';
import { PopOutPane } from '../../workspace/PopOutPane';
import { createRegistry } from '../../workspace/registry';
import { WorkspacePage } from '../../workspace/WorkspacePage';
import { useOrgData, type OrgCtx } from './data';
import { ArenaRunsPane, DeploymentsPane, SuitesPane } from './panes';
import { orgPanePath } from './paths';
import { unitAlarms, unitRate } from './units';
import '../control.css';
import '../plant/plant.css';

/**
 * `/control`: the organisation overview, display level 1 of the control plane. Every deployment is
 * a unit with its state, alarms and key figures; Arena's recent runs and test suites sit beside
 * them, so production and staging are watched from one screen. Opening a unit opens its Control.
 */

export const ORG_CONTROL_PAGE = 'control.org';

export const orgRegistry = createRegistry([
  { type: 'org.deployments', title: 'Deployments', glyph: '◫', Body: DeploymentsPane },
  { type: 'org.arena', title: 'Arena runs', glyph: '▶', Body: ArenaRunsPane },
  { type: 'org.suites', title: 'Test suites', glyph: '▦', Body: SuitesPane },
]);

/** One layout: the deployments across the top, Arena's runs beside its test suites under them. */
export function orgDefaults(): PageLayouts {
  return {
    version: 1,
    active: 'overview',
    layouts: [{
      id: 'overview', name: 'Overview', popped: [],
      root: split('org', 'col', [stackOf(['org-deployments', 'org.deployments']), split('org-arena', 'row', [stackOf(['org-runs', 'org.arena']), stackOf(['org-suites', 'org.suites'])], [1.3, 1])], [1.4, 1]),
    }],
  };
}

function Strip({ ctx }: { ctx: OrgCtx }) {
  const teams = ctx.overview?.teams ?? [];
  const running = teams.filter((t) => t.running > 0).length;
  const alarms = teams.reduce((n, t) => n + unitAlarms(t).count, 0);
  const rate = teams.reduce((n, t) => n + unitRate(t).now, 0);
  const known = !!ctx.overview;
  return (
    <ul className="ctl-strip" aria-label="Organisation status">
      <StatusTile label="Deployments">{known ? `${running} running of ${teams.length}` : 'unknown'}</StatusTile>
      <StatusTile label="Alarms" glyph={alarms ? '▲' : '○'} tone={alarms ? 'bad' : 'plain'}>{known ? (alarms ? `${alarms} raised` : 'none') : 'unknown'}</StatusTile>
      <StatusTile label="Tool calls">{known ? `${rate} this minute` : 'unknown'}</StatusTile>
      <StatusTile label="Arena" glyph={ctx.runs ? '●' : '∅'} tone={ctx.runs ? 'plain' : 'unknown'}>{ctx.runs ? `${ctx.runs.length} runs` : 'not answering'}</StatusTile>
    </ul>
  );
}

function Header({ ctx }: { ctx: OrgCtx }) {
  return (
    <div className="ctl-head">
      <div className="ctl-head-row">
        <h1 className="org-title">Control room</h1>
        <Strip ctx={ctx} />
      </div>
    </div>
  );
}

export function OrgControlPage() {
  const ctx = useOrgData();
  return (
    <main className="page control-page" aria-label="Control room">
      <WorkspacePage page={ORG_CONTROL_PAGE} registry={orgRegistry} ctx={ctx} defaults={orgDefaults} paneRoute={orgPanePath} header={<Header ctx={ctx} />} />
    </main>
  );
}

/** `/control/pane/:instance`: one popped-out pane in its own window. */
export function OrgPanePage() {
  const { instance = '' } = useParams();
  const ctx = useOrgData();
  return (
    <main className="control-popout" aria-label="Control room pane">
      <PopOutPane page={ORG_CONTROL_PAGE} registry={orgRegistry} ctx={ctx} instance={instance} />
    </main>
  );
}
