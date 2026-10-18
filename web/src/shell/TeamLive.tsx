import { Link, useSearchParams } from 'react-router-dom';
import { useEffect, useMemo, useRef, type ReactNode, type Ref } from 'react';
import { api } from '../api';
import type { Layout } from '../layout';
import type { RigState } from '../useRigStream';
import { type NameMap } from '../vocab';
import { LiveGraph, useHandoffPulses, type GraphHandle } from './LiveGraph';
import type { AgentRow } from './model';
import { Btn } from './ui';
import { plural } from '../format';
import { liveProjection, stableProjection } from '../release/live';
import { communicationEvents } from '../collaboration/mergeEvents';
import type { EventRecord } from '../generated/EventRecord';
import { usePolled } from '../usePolling';
import { SubtreeControls } from '../collaboration/SubtreeControls';
import { BudgetStatus } from '../collaboration/BudgetStatus';
import { RestoreBanner } from './RestoreBanner';
import { RunFinishedBar } from './RunFinished';
import { hasProcess } from '../status';
import { agentsPath, resultsPath, workflowPath } from '../paths';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';

/** Before history arrives: one constant, so memos keyed on it hold. */
const NO_LINKS: [never[], never[]] = [[], []];

type Props = {
  rig: string;
  state: RigState;
  agents: Record<string, AgentRow>;
  names: NameMap;
  layout?: Layout;
  selected?: string;
  onOpen: (id: string) => void;
  onAnswer: (id: string) => void;
  onSelect: (id: string) => void;
  graphRef?: Ref<GraphHandle>;
  now: number;
  /** A team with a workflow: Work evidence opens its run view. */
  workflow?: boolean;
  /** A workflow part on the graph was opened: its own page. */
  onWorkflow?: (t: WorkflowTarget) => void;
  /** Agents picked with Shift, Ctrl or Command+click (or Shift+Space), to message together. */
  picked?: string[];
  onPick?: (id: string) => void;
  /** Tools before the board's own (the Design | Run switch). */
  leadTools?: ReactNode;
};

/** Live arrows: observed messages and handoffs, refreshed every 5 seconds, plus the live feed. */
function useLiveProjection(rig: string, state: RigState, layout: Layout | undefined, subtree: string | undefined) {
  const snap = state.snapshot!;
  const links = usePolled(() => Promise.all([api.runCommunications(rig), api.managedChildren(rig)]), 5000, [rig, state.epoch]);
  const [observed, children] = links.data ?? NO_LINKS;
  // Live events arrive many times a second; the arrows change only with a message, a handoff or a
  // start or stop, so the projection reruns only then.
  const lastEvents = useRef<EventRecord[] | null>(null);
  const events = useMemo(() => communicationEvents(observed, state.feed, lastEvents.current), [observed, state.feed]);
  useEffect(() => { lastEvents.current = events; }, [events]);
  const lifecycles = snap.rig.seats.map((s) => state.seats[s.id]?.lifecycle ?? '').join();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the projection reads only each agent's lifecycle
  const seatsNow = useMemo(() => state.seats, [lifecycles]);
  const fresh = useMemo(() => liveProjection(snap.rig, seatsNow, events, layout, children, subtree), [snap.rig, seatsNow, events, layout, children, subtree]);
  // Keep the drawn objects while only counts change, so the graph does not route every arrow again.
  const previous = useRef<ReturnType<typeof liveProjection> | null>(null);
  const projection = useMemo(() => stableProjection(previous.current, fresh), [fresh]);
  useEffect(() => { previous.current = projection; }, [projection]);
  return { projection, children, observedError: links.error ? 'Communication history unavailable; showing captured live events.' : null };
}

/** None of the team's agents is running (stopped, or lost in a restart): say so once, quoting a shared reason verbatim. */
function NotRunning({ state, rig }: { state: RigState; rig: string }) {
  const views = state.snapshot!.rig.seats.map((s) => state.seats[s.id]).filter(Boolean);
  const idle = views.length > 0 && !views.some((s) => hasProcess(s.lifecycle));
  const lost = idle && state.snapshot!.desired === 'running' && views.every((s) => s.lifecycle === 'attention' || s.lifecycle === 'failed');
  if (!lost) return null;
  const reasons = [...new Set(views.map((s) => s.lifecycle_reason ?? ''))];
  const shared = reasons.length === 1 && reasons[0] ? reasons[0] : null;
  return (
    <div className="bar-note not-running tone-amber" role="status" aria-label="Not running">
      <span aria-hidden="true">◉ </span>None of this team's agents is running. {shared ? <span data-raw-evidence>{shared}</span> : <>Each agent's reason is on its card, and on the <Link to={agentsPath(rig)}>Agents</Link> page.</>}
    </div>
  );
}

/**
 * The Run board: the live graph and its small toolbar, nothing beside it. Clicking a card opens
 * its agent; Shift+click picks agents to message together. Status, Start and Stop are in the project
 * bar; what the run produced is on Results.
 */
export function TeamLive(p: Props) {
  const { rig, state, agents, names, now } = p;
  const pulses = useHandoffPulses(state.feed);
  const [searchParams, setSearchParams] = useSearchParams();
  const subtree = searchParams.get('subtree') || undefined;
  const setSubtree = (id: string) => setSearchParams((prev) => { const next = new URLSearchParams(prev); if (id) next.set('subtree', id); else next.delete('subtree'); return next; });
  const viewportKey = `${rig}:live:${subtree ?? 'all'}`;
  const snap = state.snapshot!;
  const { projection, children, observedError } = useLiveProjection(rig, state, p.layout, subtree);
  const graph = useRef<GraphHandle | null>(null);
  const views = snap.rig.seats.map((s) => state.seats[s.id]).filter(Boolean);
  const rows = snap.rig.seats.map((s) => agents[s.id]).filter(Boolean);
  const picked = useMemo(() => new Set(p.picked ?? []), [p.picked]);
  // Shift, Ctrl or Command+click on an agent picks it instead of opening it; Shift+Space does the same from the keyboard.
  const pickFrom = (e: React.MouseEvent | React.KeyboardEvent) => {
    const id = (e.target as HTMLElement).closest<HTMLElement>('[data-agent]')?.dataset.agent;
    if (!p.onPick || !id || !agents[id]) return;
    e.preventDefault();
    e.stopPropagation();
    p.onPick(id);
  };
  const setGraph = (h: GraphHandle | null) => {
    graph.current = h;
    if (typeof p.graphRef === 'function') p.graphRef(h);
    else if (p.graphRef) (p.graphRef as { current: GraphHandle | null }).current = h;
  };
  return (
    <section className="panel team-live" aria-label="Run board">
      <div className="board-toolbar" role="toolbar" aria-label="Run board">
        {p.leadTools}
        <SubtreeControls compiled={snap.rig} children={children} root={subtree} selected={p.selected} onChange={setSubtree} />
        <span className="tb-fill" />
        {p.workflow && <Link className="btn btn-ghost btn-sm" to={workflowPath(rig, 'run')}>Work evidence</Link>}
        <Btn variant="ghost" size="sm" icon="fit" onClick={() => graph.current?.fit()} kbd="f">Fit</Btn>
      </div>
      <BudgetStatus rig={rig} names={names} />
      <RestoreBanner team={rig} views={views} names={names} />
      <NotRunning state={state} rig={rig} />
      {state.gaps > 0 && <p className="bar-note gap-notice tone-amber" role="status">{plural(state.gaps, 'event')} lost — history has a gap</p>}
      {(projection.unknown > 0 || observedError) && <div className="live-evidence-note">{projection.unknown > 0 && `${projection.unknown} disconnected agents have unknown status.`} {observedError}</div>}
      <div className="team-body" onClickCapture={(e) => { if (e.shiftKey || e.ctrlKey || e.metaKey) pickFrom(e); }} onKeyDownCapture={(e) => { if (e.key === ' ' && e.shiftKey) pickFrom(e); }}>
        {projection.compiled.seats.length === 0 ? (
          <div className="release-empty">
            <h2>{subtree ? 'No live agents in this subtree' : 'No live agents'}</h2>
            <p>{subtree ? 'This branch is stopped or no longer part of the current run. Choose Whole tree to return.' : <>The board shows running agents. <Link to={agentsPath(rig)}>Agents</Link> lists every agent and what it did.</>}</p>
          </div>
        ) : (
          <LiveGraph viewportKey={viewportKey} liveOnly observedLabels={projection.labels} onWorkflow={p.onWorkflow} ref={setGraph} rig={rig} compiled={projection.compiled} agents={agents} layout={projection.layout} names={names}
            selected={p.selected} picked={picked} onOpen={p.onOpen} onAnswer={p.onAnswer} onSelect={p.onSelect} now={now} pulses={pulses} />
        )}
      </div>
      <RunFinishedBar agents={rows} href={resultsPath(rig)} />
    </section>
  );
}
