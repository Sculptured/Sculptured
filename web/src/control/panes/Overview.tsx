import { Link } from 'react-router-dom';
import { useProject } from '../../pages/ProjectContext';
import { BoardStyleSwitch } from '../../pages/BoardPage';
import { boardPath } from '../../paths';
import { ConnectingPanel } from '../../shell/ConnectingPanel';
import type { AgentRow } from '../../shell/model';
import { useShell } from '../../shell/ShellContext';
import { TeamLive } from '../../shell/TeamLive';
import { useSelection } from '../../workspace/selection';
import { useControl } from '../context';

/**
 * System overview: the team's live board (the Run board), first in the default layout. Focusing a
 * card selects its agent for every pane that follows the selection; opening it (click, Enter) also
 * brings Agent detail forward. A card's Answer brings Approvals forward.
 */
export function OverviewPane() {
  const p = useProject();
  const shell = useShell();
  const { show } = useControl();
  const { selection, select } = useSelection();
  if (!p.live) return <NotLive />;
  const agents: Record<string, AgentRow> = {};
  for (const s of p.live.snapshot!.rig.seats) if (shell.byId[s.id]) agents[s.id] = shell.byId[s.id];
  const pick = (id: string) => { shell.setSelected(id); select({ agent: id }); };
  return (
    <div className="ctl-overview">
      <TeamLive rig={p.team} state={p.live} agents={agents} names={p.row?.names ?? {}} layout={shell.layouts[p.team]} selected={selection.agent}
        onOpen={(id) => { pick(id); show('agent'); }} onAnswer={(id) => { pick(id); show('approvals'); }} onSelect={pick}
        now={shell.now} workflow={!!p.draft?.workflow} leadTools={<BoardStyleSwitch />} />
    </div>
  );
}

/** Before the team's live state arrives: connecting, or not started yet (the Design board is where it is made). */
function NotLive() {
  const p = useProject();
  if (p.started) return <ConnectingPanel key={p.team} rig={p.team} />;
  return (
    <div className="ctl-empty ctl-not-started">
      <p><span aria-hidden="true">○ </span><strong>Not started yet.</strong> The live board shows here once the team runs. Start it with Start team above.</p>
      {p.builtHere && <p><Link className="btn btn-secondary btn-sm" to={boardPath(p.team, 'design')}>Open the Design board</Link></p>}
    </div>
  );
}
