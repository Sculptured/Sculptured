import { useParams } from 'react-router-dom';
import { Icon } from '../shell/ui';
import { PopOutPane } from '../workspace/PopOutPane';
import { WorkspacePage } from '../workspace/WorkspacePage';
import { useArenaData } from './data';
import { arenaDefaults } from './defaults';
import { arenaPanePath } from './paths';
import { arenaRegistry } from './registry';
import '../tests/tests.css';
import './arena.css';

/** The workspace's name for its layouts and its selection channel. */
export const ARENA_PAGE = 'arena';

function Header() {
  return (
    <div className="arena-head">
      <h1 className="page-title"><Icon name="flask" size={18} /> Arena control</h1>
      <p className="muted tst-small">Run a team in a simulated world saved at every step. Go back to any step, change it, and rerun from there.</p>
    </div>
  );
}

/** `/arena`: Arena's runs, forks, world, saves and health, as panes the operator can arrange. */
export function ArenaControlPage() {
  const ctx = useArenaData();
  return (
    <main className="arena-page">
      <WorkspacePage page={ARENA_PAGE} registry={arenaRegistry} ctx={ctx} defaults={arenaDefaults} paneRoute={arenaPanePath} header={<Header />} />
    </main>
  );
}

/** `/arena/pane/:instance`: one popped-out pane in its own window, without the app's bars. */
export function ArenaPanePage() {
  const { instance = '' } = useParams();
  const ctx = useArenaData();
  return <main className="arena-page arena-popout"><PopOutPane page={ARENA_PAGE} registry={arenaRegistry} ctx={ctx} instance={instance} /></main>;
}
