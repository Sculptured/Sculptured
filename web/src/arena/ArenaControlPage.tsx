import { useParams } from 'react-router-dom';
import { Icon } from '../shell/ui';
import { PopOutPane } from '../workspace/PopOutPane';
import { WorkspacePage } from '../workspace/WorkspacePage';
import { isArenaDown, useArenaData, type ArenaCtx } from './data';
import { arenaDefaults } from './defaults';
import { ArenaDownBanner } from './gate';
import { arenaPanePath } from './paths';
import { arenaRegistry } from './registry';
import { StatusStrip } from './StatusStrip';
import '../tests/tests.css';
import './arena.css';

/** The workspace's name for its layouts and its selection channel. */
export const ARENA_PAGE = 'arena';

/** While Arena is not answering, one alarm for the whole page; the panes only say "Offline". */
function DownBanner({ ctx }: { ctx: ArenaCtx }) {
  return ctx.health === null && isArenaDown(ctx.healthError) ? <ArenaDownBanner error={ctx.healthError} /> : null;
}

function Header({ ctx }: { ctx: ArenaCtx }) {
  return (
    <>
      <div className="arena-head">
        <h1 className="arena-title"><Icon name="flask" size={16} /> Arena control</h1>
        <StatusStrip ctx={ctx} />
      </div>
      <DownBanner ctx={ctx} />
    </>
  );
}

/** `/arena`: Arena's runs, forks, world, saves and health, as panes the operator can arrange. */
export function ArenaControlPage() {
  const ctx = useArenaData();
  return (
    <main className="arena-page">
      <WorkspacePage page={ARENA_PAGE} registry={arenaRegistry} ctx={ctx} defaults={arenaDefaults} paneRoute={arenaPanePath} header={<Header ctx={ctx} />} />
    </main>
  );
}

/** `/arena/pane/:instance`: one popped-out pane in its own window, without the app's bars. */
export function ArenaPanePage() {
  const { instance = '' } = useParams();
  const ctx = useArenaData();
  return (
    <main className="arena-page arena-popout">
      <DownBanner ctx={ctx} />
      <PopOutPane page={ARENA_PAGE} registry={arenaRegistry} ctx={ctx} instance={instance} />
    </main>
  );
}
