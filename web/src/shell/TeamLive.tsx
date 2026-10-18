import { track } from '../hosted/analytics';
import { hasProcess, needsAttention } from '../status';
import {DismissibleError} from './DismissibleError';
import { StartRefused } from './StartRefused';
import { errorText, issuesOf, STARTING_REASON } from '../errors';
import { Link, useSearchParams } from 'react-router-dom';
import { useEffect, useMemo, useRef, useState, type Ref } from 'react';
import { api, type StartResult } from '../api';
import type { StopFailure } from '../generated/StopFailure';
import type { StopRigOutcome } from '../generated/StopRigOutcome';
import type { DraftIssue } from '../generated/DraftIssue';
import { ModeTabs } from '../structure/StructureBoard';
import { useStartConfirm, type StartConfirmation } from '../structure/StartConfirm';
import type { Layout } from '../layout';
import type { RigState } from '../useRigStream';
import { useRequestOnce } from '../useRequestOnce';
import { nameForId, WORDS, type NameMap } from '../vocab';
import { SpecNotes, specNotes } from '../components/SpecNotes';
import { structurePath } from './model';
import { AgentList } from './AgentList';
import { LiveGraph, useHandoffPulses, type GraphHandle } from './LiveGraph';
import type { AgentRow } from './model';
import { Btn, Icon, Segmented } from './ui';
import { plural } from '../format';
import type { TeamDraft } from '../generated/TeamDraft';
import { liveProjection, stableProjection } from '../release/live';
import { teamTokensText } from '../herdr';

/** Before history arrives: one constant, so memos keyed on it hold. */
const NO_LINKS: [never[], never[]] = [[], []];
import { communicationEvents } from '../collaboration/mergeEvents';
import type { EventRecord } from '../generated/EventRecord';
import { usePolled } from '../usePolling';
import { WorkflowInspector } from '../workflow/WorkflowInspector';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';
import { CommunicationDashboard } from '../collaboration/CommunicationDashboard';
import { UsageDashboard } from '../collaboration/UsageDashboard';
import { WhereTokensWent } from '../collaboration/WhereTokensWent';
import { KnowledgeView } from '../collaboration/KnowledgeView';
import { isLearned, seenAt } from '../knowledge';
import { DeploymentHistoryView } from '../collaboration/DeploymentHistoryView';
import { SubtreeControls } from '../collaboration/SubtreeControls';
import { BudgetStatus } from '../collaboration/BudgetStatus';
import { RestoreBanner } from './RestoreBanner';
import { TeamConversation } from '../collaboration/TeamConversation';
import { useConversationView } from '../collaboration/view';

export type Mode = 'graph' | 'list';

type Props = {
  draft?: TeamDraft;
  rig: string;
  state: RigState;
  connected: boolean;
  agents: Record<string, AgentRow>;
  names: NameMap;
  layout?: Layout;
  mode: Mode;
  onMode: (m: Mode) => void;
  selected?: string;
  onOpen: (id: string) => void;
  onAnswer: (id: string) => void;
  onSelect: (id: string) => void;
  graphRef?: Ref<GraphHandle>;
  now: number;
  /** What the team is called in sentences. Defaults to the internal name. */
  title?: string;
  /** Stops the team; the outcome lists the agents that could not be stopped. */
  onStopTeam?: (requestId: string) => Promise<StopRigOutcome>;
  /** A team built here: the header switches between its Structure and Live boards. */
  teamId?: string;
  /** Warnings from the latest Start, shown until dismissed. */
  startWarnings?: DraftIssue[];
  onDismissWarnings?: () => void;
  /** Starts the team again (a team built here whose agents have no process). */
  /** `overPlanLimit`: start past a plan hold (the person chose Start anyway). */
  onStartTeam?: (requestId: string, resume: boolean, overPlanLimit?: boolean) => Promise<StartResult>;
  /** Asked before Start when the team has real (non-Practice) agents. */
  startConfirm?: StartConfirmation | null;
};

/** Lifecycles in which an agent has, or may have, a process. */

/** The Live Team Graph panel: header, notices, then the graph or its list fallback. */
export function TeamLive(p: Props) {
  const { rig, state, connected, agents, names, mode, onMode, now } = p;
  const pulses = useHandoffPulses(state.feed);
  const shown = p.title ?? rig;
  const [analytics,setAnalytics]=useState(false);
  const [usageOpen,setUsageOpen]=useState(false);
  const usageButton=useRef<HTMLButtonElement>(null);
  const [tokensOpen,setTokensOpen]=useState(false);
  const [knowledgeOpen,setKnowledgeOpen]=useState(false);
  const knowledgeButton = useRef<HTMLButtonElement>(null);
  // Lessons agents recorded since this browser last opened Knowledge: a quiet count, never a prompt.
  const lessons = usePolled(() => api.decisions(rig), 120000, [rig, knowledgeOpen]);
  const newLessons = knowledgeOpen ? 0 : (lessons.data ?? []).filter((e) => isLearned(e) && e.at_ms > seenAt(rig)).length;
  const tokensButton=useRef<HTMLButtonElement>(null);
  const analyticsButton=useRef<HTMLButtonElement>(null);
  const [confirmStop, setConfirmStop] = useState(false);
  const [{ open: conversationOpen }, changeConversation] = useConversationView(rig);
  const conversationButton = useRef<HTMLButtonElement>(null);
  const closeConversation = () => { changeConversation({ open: false }); conversationButton.current?.focus(); };
  const [workflowTarget, setWorkflowTarget] = useState<WorkflowTarget | null>(null);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stopFailures, setStopFailures] = useState<StopFailure[]>([]);
  const stopBtn = useRef<HTMLButtonElement>(null);
  const cancelBtn = useRef<HTMLButtonElement>(null);
  const wasConfirming = useRef(false);
  // One id per stop: a double click reuses it. It is renewed once the server answers.
  const stopOnce = useRequestOnce();
  const graph = useRef<GraphHandle | null>(null);

  // Focus moves into the inline confirm, and back to the trigger when it closes.
  useEffect(() => {
    if (confirmStop) cancelBtn.current?.focus();
    else if (wasConfirming.current) stopBtn.current?.focus();
    wasConfirming.current = confirmStop;
  }, [confirmStop]);

  const [searchParams, setSearchParams] = useSearchParams();
  const historical = searchParams.get('view') === 'history';
  const setHistorical = (value: boolean) => setSearchParams(previous => {const next=new URLSearchParams(previous);if(value)next.set('view','history');else next.delete('view');return next;});
  const subtree = searchParams.get('subtree') || undefined;
  const setSubtree = (id: string) => setSearchParams(previous => {
    const next = new URLSearchParams(previous);
    if(id) next.set('subtree',id); else next.delete('subtree');
    return next;
  });
  const snap = state.snapshot!;
  const links = usePolled(() => Promise.all([api.runCommunications(rig), api.managedChildren(rig)]), 5000, [rig, state.epoch]);
  const [observed, children] = links.data ?? NO_LINKS;
  const observedError = links.error && 'Communication history unavailable; showing captured live events.';
  // Live events arrive many times a second; the arrows change only with a message, a handoff or a
  // start or stop, so the projection reruns only then.
  const lastEvents = useRef<EventRecord[] | null>(null);
  const events = useMemo(() => communicationEvents(observed, state.feed, lastEvents.current), [observed, state.feed]);
  useEffect(() => { lastEvents.current = events; }, [events]);
  const lifecycles = snap.rig.seats.map((s) => state.seats[s.id]?.lifecycle ?? '').join();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the projection reads only each agent's lifecycle
  const seatsNow = useMemo(() => state.seats, [lifecycles]);
  const fresh = useMemo(() => liveProjection(snap.rig, seatsNow, events, p.layout, children, subtree), [snap.rig, seatsNow, events, p.layout, children, subtree]);
  // Keep the drawn objects while only counts change, so the graph does not route every arrow again.
  const previous = useRef<ReturnType<typeof liveProjection> | null>(null);
  const projection = useMemo(() => stableProjection(previous.current, fresh), [fresh]);
  useEffect(() => { previous.current = projection; }, [projection]);
  const views = snap.rig.seats.map((s) => state.seats[s.id]).filter(Boolean);
  const running = views.filter((s) => s.lifecycle === 'running').length;
  const attention = views.filter((s) => needsAttention(s.lifecycle)).length;
  const anyLive = views.some((s) => hasProcess(s.lifecycle) || s.lifecycle === 'attention');
  // A team built here is starting while any agent waits to launch: the server refuses a stop then.
  const starting = !!p.teamId && snap.desired === 'running' && views.some((s) => s.lifecycle === 'launching' || s.lifecycle === 'planned');
  const stopReason = starting ? STARTING_REASON : !anyLive && snap.desired === 'stopped' ? 'Already stopped.' : null;
  // Nothing has a process (stopped, or lost in a restart): the team can be started again from here.
  const idle = !starting && views.length > 0 && !views.some((s) => hasProcess(s.lifecycle));
  const canStart = !!p.onStartTeam && idle;
  // Every agent was lost the same way (a restart): say so once, quoting the reason verbatim.
  const lost = idle && snap.desired === 'running' && views.every((s) => s.lifecycle === 'attention' || s.lifecycle === 'failed');
  const reasons = [...new Set(views.map((s) => s.lifecycle_reason ?? ''))];
  const sharedReason = reasons.length === 1 && reasons[0] ? reasons[0] : null;
  const [startingNow, setStartingNow] = useState(false);
  const [startError, setStartError] = useState<{ text: string; issues: DraftIssue[]; code?: string; resume: boolean } | null>(null);
  const startOnce = useRequestOnce();
  const confirmStart = useStartConfirm();
  const startTeam = async (resume = false, overPlanLimit = false) => {
    if (!p.onStartTeam) return;
    const onStart = p.onStartTeam;
    setStartError(null);
    setStartingNow(true);
    try {
      if (overPlanLimit) await onStart('', resume, true);
      else await startOnce((id) => onStart(id, resume));
    } catch (e) {
      setStartError({ text: errorText(e), issues: issuesOf(e), code: (e as { code?: string } | null)?.code, resume });
    } finally {
      setStartingNow(false);
    }
  };

  const stopRig = async () => {
    setError(null);
    setStopFailures([]);
    setStopping(true);
    try {
      const outcome = await stopOnce((id) => (p.onStopTeam ? p.onStopTeam(id) : api.stopRig(rig, id)));
      setConfirmStop(false);
      setStopFailures(outcome?.failed ?? []);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStopping(false);
    }
  };

  /** Every agent at once, like Ctrl+C: interrupted now, forced after 2 seconds. */
  const stopNow = async () => {
    setError(null);
    setStopping(true);
    try {
      await api.stopTeamNow(rig);
      setConfirmStop(false);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStopping(false);
    }
  };

  const setGraph = (h: GraphHandle | null) => {
    graph.current = h;
    if (typeof p.graphRef === 'function') p.graphRef(h);
    else if (p.graphRef) (p.graphRef as { current: GraphHandle | null }).current = h;
  };

  const timelineTabs = <Segmented label="Timeline" value={historical ? 'history' : 'current'} onChange={value=>setHistorical(value==='history')} options={[{value:'current',label:'Current'},{value:'history',label:'History'}]} />;
  if (historical) return <section className="panel team-live" aria-label={`Team ${shown}`}>
    <div className="panel-bar"><span className="panel-title">{p.teamId ? <ModeTabs teamId={p.teamId} mode="live" started name={shown}/> : `Live · ${shown}`}</span><span className="panel-rule"/>{timelineTabs}</div>
    <DeploymentHistoryView rig={rig}/>
  </section>;
  return (
    <section className="panel team-live" aria-label={`Team ${shown}`}>
      <div className="panel-bar">
        <span className="panel-title">
          {p.teamId ? (
            <ModeTabs teamId={p.teamId} mode="live" started name={shown} />
          ) : (
            <>
              <Icon name="activity" size={16} />
              <span className="tab-on">Run</span>
              <span className="muted"> · </span>
              <span className="team-title">{shown}</span>
            </>
          )}
        </span>
        <span className="panel-rule" aria-hidden="true" />
        <span className="panel-tools">
          <Segmented
            label="View"
            value={mode}
            onChange={(m) => { track(m === 'graph' ? 'switch-graph' : 'switch-list'); onMode(m); }}
            options={[
              { value: 'graph', label: 'Graph', icon: 'graph', kbd: 'l', tip: 'Graph view' },
              { value: 'list', label: 'List', icon: 'list', kbd: 'l', tip: 'List view' },
            ]}
          />
          <details className="more-menu">
            <summary>More{newLessons > 0 && <span className="quiet-count" aria-label={`${newLessons} new ${newLessons === 1 ? 'lesson' : 'lessons'}`}> · {newLessons}</span>}</summary>
            {/* Picking an item closes the menu. */}
            <div className="more-items" onClick={(e) => { if ((e.target as HTMLElement).closest('button')) e.currentTarget.closest('details')?.removeAttribute('open'); }}>
              <Btn ref={conversationButton} aria-expanded={conversationOpen} onClick={() => changeConversation({ open: !conversationOpen })} icon="message" tip="Follow agent updates, handoffs and questions">Conversation</Btn>
              <Btn ref={usageButton} aria-expanded={usageOpen} onClick={()=>{if(!usageOpen)track('open-token-use');setUsageOpen(!usageOpen);setAnalytics(false);setTokensOpen(false)}} tip="Tokens each agent has used over time">Token use</Btn>
              <Btn ref={tokensButton} aria-expanded={tokensOpen} onClick={()=>{setTokensOpen(!tokensOpen);setUsageOpen(false);setAnalytics(false)}} tip="What this run's tokens were spent on: setup, files, handoffs, the plan window">Where tokens went</Btn>
              <Btn ref={analyticsButton} aria-expanded={analytics} onClick={()=>{setAnalytics(!analytics);setUsageOpen(false);setTokensOpen(false)}}>Communications</Btn>
              <Btn ref={knowledgeButton} aria-expanded={knowledgeOpen} onClick={()=>{setKnowledgeOpen(!knowledgeOpen);setUsageOpen(false);setTokensOpen(false);setAnalytics(false)}} tip="What the agents know: lessons they recorded, your decisions, and the project map made at each start">Knowledge{newLessons > 0 ? ` · ${newLessons} new` : ''}</Btn>
              <Btn onClick={() => setHistorical(true)}>History</Btn>
              {p.draft?.workflow && <Btn onClick={() => setWorkflowTarget('run')}>Work evidence</Btn>}
              {mode === 'graph' && <Btn icon="fit" onClick={() => graph.current?.fit()} kbd="f">Fit graph</Btn>}
            </div>
          </details>
          <details className="team-status">
            <summary> Status{attention > 0 ? ` · ${attention} need you` : !connected ? ' · reconnecting' : state.gaps > 0 ? ' · history gap' : ''}</summary>
          <div className="team-info">
            <span className="rig-counts">
              {WORDS.desired(snap.desired)} · {WORDS.teamCounts(running, snap.rig.seats.length)}
              {teamTokensText(views) && <span className="team-tokens"> · {teamTokensText(views)}</span>}
              {attention > 0 ? <span className="tone-red"> · {WORDS.needAttention(attention)}</span> : ''}
              {running > 0 && views.every(s => !s.current_turn && !s.current_tool && s.activity === 'idle') && <span> · No active work; sessions are open. Stop team closes them.</span>}
            </span>
            {!connected && (
              <span role="status" className="tone-amber">
                {' '}
                · reconnecting…
              </span>
            )}
            {state.gaps > 0 && (
              <span className="gap-notice tone-amber" role="status">
                {plural(state.gaps, 'event')} lost — history has a gap
              </span>
            )}
            {specNotes(snap.rig.diagnostics).length > 0 && (
              <details className="spec-advanced" data-advanced>
                <summary>{WORDS.setupNotes}</summary>
                <SpecNotes diagnostics={snap.rig.diagnostics} />
              </details>
            )}
          </div>
          </details>
          {canStart && (
            <Btn variant="primary" icon="play" className="start-btn" onClick={() => confirmStart.ask(p.startConfirm ?? null, (c) => void startTeam(c.resume))} disabled={startingNow} aria-label="Start team">
              <span>
                Start<span className="wide-only"> team</span>
              </span>
            </Btn>
          )}
          {!confirmStop && (anyLive || starting) && (
            <>
              <Btn
                danger
                icon="stop"
                ref={stopBtn}
                onClick={() => setConfirmStop(true)}
                disabled={!!stopReason}
                aria-describedby={stopReason ? `stop-team-why-${rig}` : undefined}
                aria-label={WORDS.stopTeam}
              >
                <span>
                  Stop<span className="wide-only"> team</span>…
                </span>
              </Btn>
              {stopReason && (
                <span id={`stop-team-why-${rig}`} className="reason">
                  {stopReason}
                </span>
              )}
            </>
          )}
        </span>
      </div>
      <BudgetStatus rig={rig} names={names} />
      <RestoreBanner team={rig} views={views} names={names} />
      {lost && (
        <div className="bar-note not-running tone-amber" role="status" aria-label="Not running">
          <p>
            <span aria-hidden="true">◉ </span>None of this team's agents is running.{' '}
            {sharedReason ? (
              <span data-raw-evidence>{sharedReason}</span>
            ) : (
              <>Each agent's reason is on its card.</>
            )}
          </p>
          {canStart && <p className="muted">Press Start team to launch every agent again. Each agent gets its job again as its first message.</p>}
        </div>
      )}
      <StartRefused refusal={startError} onClose={() => setStartError(null)} onAnyway={() => void startTeam(startError?.resume ?? false, true)}>
        {startError && startError.code !== 'plan_limit' && p.teamId && <p>Fix it on the <Link to={structurePath(p.teamId)}>Design board</Link>.</p>}
      </StartRefused>
      {p.startWarnings && p.startWarnings.length > 0 && (
        <div className="bar-note start-warnings tone-amber" role="status" aria-label="Start warnings">
          <p>
            Started with {p.startWarnings.length === 1 ? 'a warning' : `${p.startWarnings.length} warnings`}:
          </p>
          <ul>
            {p.startWarnings.map((w, i) => (
              <li key={i}>
                <span aria-hidden="true">△ </span>
                {w.message}
              </li>
            ))}
          </ul>
          {p.onDismissWarnings && <Btn onClick={p.onDismissWarnings}>Dismiss</Btn>}
        </div>
      )}
      {confirmStop && (
        <div
          role="group"
          aria-label={WORDS.confirmStopTeam}
          className="confirm bar-confirm"
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.stopPropagation();
              setConfirmStop(false);
            }
          }}
        >
          <p>{WORDS.stopTeamConsequence(shown)}</p>
          <Btn danger onClick={stopRig} disabled={stopping} aria-describedby={stopping ? 'stop-rig-reason' : undefined}>
            {WORDS.confirmStopTeam}
          </Btn>
          <Btn danger onClick={stopNow} disabled={stopping} tip="Interrupts every agent and the commands it is running at once, like Ctrl+C in a terminal; anything still running after 2 seconds is ended.">
            Stop everything now
          </Btn>
          {stopping && (
            <span id="stop-rig-reason" className="reason">
              {WORDS.stoppingTeam(shown)}
            </span>
          )}
          <Btn ref={cancelBtn} onClick={() => setConfirmStop(false)}>
            Cancel
          </Btn>
        </div>
      )}
      {error && (
        <DismissibleError className="tone-red bar-note">
          {error}
        </DismissibleError>
      )}
      {stopFailures.length > 0 && (
        <DismissibleError className="error stop-failures bar-note">
          <p>{WORDS.agentsNotStopped(stopFailures.length)}</p>
          <ul aria-label={WORDS.agentsNotStoppedLabel}>
            {stopFailures.map((f) => (
              <li key={f.seat}>
                {nameForId(f.seat, names)} — <span data-raw-evidence>{f.reason}</span>
              </li>
            ))}
          </ul>
        </DismissibleError>
      )}
      <SubtreeControls compiled={snap.rig} children={children} root={subtree} selected={p.selected} onChange={setSubtree} />
      {(projection.unknown > 0 || observedError) && <div className="live-evidence-note">{projection.unknown>0&&`${projection.unknown} disconnected agents have unknown status.`} {observedError}</div>}
      <div className="team-body">
        {mode === 'list' ? (
          <AgentList compiled={snap.rig} agents={agents} names={names} selected={p.selected} onOpen={p.onOpen} now={now} connections={false} />
        ) : projection.compiled.seats.length===0 ? <div className="release-empty"><h2>{subtree ? "No live agents in this subtree" : "No live agents"}</h2><p>{subtree ? "This branch is stopped or no longer part of the current run. Choose Whole tree to return." : "The graph shows running agents. Switch to List to see every agent and what it did."}</p></div> : (
          <LiveGraph
            viewportKey={`${rig}:live:${subtree ?? "all"}`}
            liveOnly observedLabels={projection.labels}
            onWorkflow={setWorkflowTarget}
            ref={setGraph}
            rig={rig}
            compiled={projection.compiled}
            agents={agents}
            layout={projection.layout}
            names={names}
            selected={p.selected}
            onOpen={p.onOpen}
            onAnswer={p.onAnswer}
            onSelect={p.onSelect}
            now={now}
            pulses={pulses}
          />
        )}
        {analytics && <CommunicationDashboard rig={rig} state={state} connected={connected} names={names} onClose={()=>{setAnalytics(false);analyticsButton.current?.focus()}}/>}
        {usageOpen && !analytics && <UsageDashboard rig={rig} names={names} onClose={()=>{setUsageOpen(false);usageButton.current?.focus()}}/>}
        {knowledgeOpen && !analytics && !usageOpen && !tokensOpen && <KnowledgeView rig={rig} onClose={()=>{setKnowledgeOpen(false);knowledgeButton.current?.focus()}}/>}
        {tokensOpen && !analytics && !usageOpen && <WhereTokensWent rig={rig} seats={Object.keys(agents)} onClose={()=>{setTokensOpen(false);tokensButton.current?.focus()}}/>}
        {conversationOpen && !analytics && !usageOpen && !tokensOpen && !knowledgeOpen && <TeamConversation key={rig} rig={rig} state={state} connected={connected} names={names} agents={agents} onOpen={p.onOpen} onAnswer={p.onAnswer} onClose={closeConversation} />}
      </div>
      {p.draft?.workflow && workflowTarget && <WorkflowInspector draft={p.draft} target={workflowTarget} locked edit={() => {}} onOpen={id => { const seat = snap.rig.seats.find(s => s.member === id); if (seat) p.onOpen(seat.id); }} onClose={() => setWorkflowTarget(null)} />}
      {confirmStart.dialog}
    </section>
  );
}
