import { useEffect, useId } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { seatIdFor } from '../draft/ids';
import { agentPagePath, workflowPath, type BoardMode } from '../paths';
import { RunSelectionBar } from '../shell/Broadcast';
import { ConnectingPanel } from '../shell/ConnectingPanel';
import type { AgentRow } from '../shell/model';
import { useShell } from '../shell/ShellContext';
import { TeamLive } from '../shell/TeamLive';
import { Icon, Segmented } from '../shell/ui';
import { BOARD_STYLES, setBoardStyle, useBoardStyle, type BoardStyle } from '../shell/boardStyle';
import { StructureBoard } from '../structure/StructureBoard';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';
import { useProject, type Project } from './ProjectContext';
import { startReasonOf } from './useProjectRun';

/** Why Run is off before a team's first start. */
export const RUN_NOT_YET = 'Run opens once the team has started';
const NO_DESIGN = 'This team was started from a file: it has no design to edit here';

/** The mode to show: the one asked for when it can show, else Run while the team runs, else Design. */
export function boardMode(asked: string | null, p: Pick<Project, 'builtHere' | 'started' | 'running' | 't' | 'live'>): BoardMode {
  // A design that could not be loaded still opens on Design, which says why.
  if (asked === 'design' && (p.builtHere || !!p.t.loadError) && !p.live) return 'design';
  if (asked === 'design' && p.builtHere) return 'design';
  if (asked === 'run' && p.started) return 'run';
  // A team set to run (even one whose agents were lost in a restart) opens on Run.
  return p.running || p.live?.snapshot?.desired === 'running' || !p.builtHere ? 'run' : 'design';
}

/** Design | Run: the board's two modes. The one that cannot open says why, on hover, focus and to screen readers. */
function ModeSwitch({ mode, p, onChange }: { mode: BoardMode; p: Project; onChange: (m: BoardMode) => void }) {
  const ids = useId();
  const option = (m: BoardMode, label: string, icon: 'graph' | 'activity', off: string | null) => (
    <button type="button" className={`btn btn-ghost btn-sm${mode === m ? ' on' : ''}`} aria-pressed={mode === m} disabled={!!off}
      aria-describedby={off ? `${ids}-${m}` : undefined} data-tip={off ?? undefined} onClick={() => onChange(m)}>
      <Icon name={icon} size={14} />{label}
      {off && <span id={`${ids}-${m}`} className="sr-only">{off}</span>}
    </button>
  );
  return (
    <span role="group" aria-label="Board mode" className="seg mode-switch" data-tour="board-mode">
      {option('design', 'Design', 'graph', p.builtHere ? null : NO_DESIGN)}
      {option('run', 'Run', 'activity', p.started ? null : RUN_NOT_YET)}
    </span>
  );
}

/** Board style: Cards | Logos. Per browser; the same agents stay at the same places. */
export function BoardStyleSwitch() {
  const style = useBoardStyle();
  return (
    <Segmented<BoardStyle>
      label="Board style"
      className="style-switch"
      value={style}
      onChange={setBoardStyle}
      options={[
        { value: 'cards', label: BOARD_STYLES.cards, icon: 'grid', tip: 'Board style: each agent as a card with its details' },
        { value: 'logos', label: BOARD_STYLES.logos, icon: 'sparkles', tip: "Board style: each agent as its provider's logo, moving while it works" },
      ]}
    />
  );
}

/**
 * `/p/:team/board`: the board, full width, in Design or Run. A card opens its agent's page; coming
 * back keeps the board's view and selection.
 */
export function BoardPage() {
  const p = useProject();
  const shell = useShell();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const mode = boardMode(params.get('mode'), p);
  const { setBoardView } = shell;
  useEffect(() => {
    setBoardView(mode === 'design' ? 'structure' : 'team');
    return () => setBoardView(null);
  }, [mode, setBoardView]);
  const startReason = startReasonOf(p);
  const lead = <><ModeSwitch mode={mode} p={p} onChange={(m) => setParams((prev) => { const next = new URLSearchParams(prev); next.set('mode', m); return next; }, { replace: true })} /><BoardStyleSwitch /></>;
  const toWorkflow = (t: WorkflowTarget | 'list') => navigate(workflowPath(p.team, typeof t === 'object' ? `stage:${t.stage}` : t));
  if (mode === 'design') {
    const openAgent = (id: string) => {
      const seat = p.draft ? seatIdFor(p.draft, id) : null;
      if (seat) navigate(agentPagePath(p.team, seat));
    };
    return (
      <main className="page page-board">
        <StructureBoard ref={shell.refs.board} teamId={p.team} t={p.t} running={p.live ? p.running : null} onOpenAgent={openAgent} onWorkflow={toWorkflow} startReason={startReason} leadTools={lead} onStartBlock={p.setStartBlock} />
      </main>
    );
  }
  return <main className="page page-board"><RunBoard p={p} lead={lead} onWorkflow={toWorkflow} /></main>;
}

function RunBoard({ p, lead, onWorkflow }: { p: Project; lead: React.ReactNode; onWorkflow: (t: WorkflowTarget) => void }) {
  const shell = useShell();
  if (!p.live) return <ConnectingPanel key={p.team} rig={p.team} />;
  const agents: Record<string, AgentRow> = {};
  for (const s of p.live.snapshot!.rig.seats) if (shell.byId[s.id]) agents[s.id] = shell.byId[s.id];
  const picked = shell.picked;
  return (
    <>
      <TeamLive rig={p.team} state={p.live} agents={agents} names={p.row?.names ?? {}} layout={shell.layouts[p.team]} selected={shell.selected}
        onOpen={(id) => shell.open({ rig: p.team, id })} onAnswer={(id) => shell.answer(p.team, id)} onSelect={shell.setSelected}
        graphRef={shell.refs.graph} now={shell.now} workflow={!!p.draft?.workflow} onWorkflow={onWorkflow} picked={picked} onPick={shell.pick} leadTools={lead} />
      {picked.length > 0 && <RunSelectionBar picked={picked.map((id) => shell.byId[id])} onMessage={() => shell.broadcast(p.team, picked)} onClear={() => shell.setPicked([])} />}
    </>
  );
}
