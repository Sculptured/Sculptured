import {DismissibleError} from '../shell/DismissibleError';
import { StartRefused } from '../shell/StartRefused';
import { RenameProject } from './RenameProject';
import { errorText } from '../errors';
import { useEffect, useId, useImperativeHandle, useMemo, useRef, useState, type ReactNode, type Ref } from 'react';
import { Link } from 'react-router-dom';
import { api, ApiError } from '../api';
import type { AgentKind } from '../generated/AgentKind';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import { addAgent, selectionExists, type Selection } from '../draft/edits';
import { nextFreeSlot } from '../draft/geometry';
import { errorsOf, liveIssues, MARK, selectionFor, startReason } from '../draft/issues';
import { teamPath } from '../shell/model';
import { Badge, Btn, Icon, VendorMark } from '../shell/ui';
import { useTeamDraft, type SaveState } from '../useTeamDraft';
import { useRequestOnce } from '../useRequestOnce';
import { isTextField } from '../dom';
import { AgentSheet } from './AgentSheet';
import { OFFERED_KINDS } from './ModelControls';
import { AgentPanel, ConnectionPanel, GroupPanel, kindVendor, LOCK_REASON, TeamPanel, type Edit } from './Panels';
import { StructureCanvas, type CanvasHandle } from './StructureCanvas';
import { TeamAdvanced } from './TeamAdvanced';
import { useHome } from './FolderPicker';
import { WorkflowInspector } from '../workflow/WorkflowInspector';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';
import { startConfirmation, useStartConfirm } from './StartConfirm';
import { useDesignAssistant } from './assistant/useDesignAssistant';
import { DesignAssistant, ProposalBar, WorkingBar } from './assistant/DesignAssistant';
import { compareDesigns } from './assistant/compare';

export type BoardHandle = { fit: () => void; zoomBy: (f: number) => void; addKey: () => void };

type Props = {
  teamId: string;
  /** From the team's live stream: true while any agent may be running; null when there is no stream. */
  running: boolean | null;
  /** The team has been started before, so the Live board has something to show. */
  started: boolean;
  narrow: boolean;
  detailsShown?: boolean;
  onShowDetails?: () => void;
  /** An agent to select once the draft is loaded (a sidebar row of a team not started). */
  initialAgent?: string | null;
  onDraft?: (d: TeamDraft) => void;
  /** The team could not be loaded (the reason), or it loaded (null). */
  onLoadError?: (reason: string | null) => void;
  onStarted: (warnings: DraftIssue[]) => void;
  ref?: Ref<BoardHandle>;
};

function SaveStatus({ state, error, onRetry, onReload, onKeepMine }: { state: SaveState; error: string | null; onRetry: () => void; onReload: () => void; onKeepMine: () => void }) {
  if (state === 'saved')
    return (
      <span className="save-state tone-green" role="status">
        <span aria-hidden="true">✓ </span>saved
      </span>
    );
  if (state === 'unsaved' || state === 'saving')
    return (
      <span className="save-state muted" role="status">
        <span aria-hidden="true">◌ </span>saving…
      </span>
    );
  return (
    <DismissibleError className="save-state tone-red">
      <span aria-hidden="true">▲ </span>not saved: {error}
      {state === 'conflict' ? (
        <>
          {' '}
          <Btn size="sm" icon="refresh" onClick={onReload}>Reload</Btn> <Btn size="sm" onClick={onKeepMine}>Keep mine</Btn>
        </>
      ) : (
        <>
          {' '}
          <Btn size="sm" icon="refresh" onClick={onRetry}>Retry</Btn>
        </>
      )}
    </DismissibleError>
  );
}

/** `[Structure] [Live] [Progress]`: the views of one team. Live needs a team that has been started. */
export function ModeTabs({ teamId, mode, started, name, title }: { teamId: string; mode: 'structure' | 'live'; started: boolean; name: string; title?: ReactNode }) {
  const ids = useId();
  return (
    <span className="mode-tabs">
      <nav aria-label="Board" className="mode-nav">
        {mode === 'structure' ? (
          <span className="mode-tab on" aria-current="page">
            <Icon name="graph" size={14} />
            Design
          </span>
        ) : (
          <Link className="mode-tab" to={`/team/${encodeURIComponent(teamId)}/structure`}>
            <Icon name="graph" size={14} />
            Design
          </Link>
        )}
        {mode === 'live' ? (
          <span className="mode-tab on" aria-current="page">
            <Icon name="activity" size={14} />
            Run
          </span>
        ) : started ? (
          <Link className="mode-tab" to={teamPath(teamId)}>
            <Icon name="activity" size={14} />
            Run
          </Link>
        ) : (
          <span className="mode-tab off" role="link" aria-disabled="true" aria-describedby={`${ids}-live`}>
            <Icon name="activity" size={14} />
            Run
          </span>
        )}
      </nav>
      {mode !== 'live' && !started && (
        <span id={`${ids}-live`} className="muted mode-note">
          Run opens once the team has started
        </span>
      )}
      <span className="mode-sep" aria-hidden="true">
        ·
      </span>
      {title ?? <span className="team-title">{name}</span>}
    </span>
  );
}

/** The narrow-screen editor: the same edits as a simple form, one agent per section. */
function FormEditor({
  draft,
  issues,
  edit,
  locked,
  onAdd,
  advanced,
}: {
  draft: TeamDraft;
  issues: DraftIssue[];
  edit: ReturnType<typeof useTeamDraft>['edit'];
  locked: boolean;
  onAdd: (k: AgentKind) => void;
  advanced?: React.ReactNode;
}) {
  const ids = useId();
  return (
    <div className="form-editor">
      <TeamPanel draft={draft} edit={edit} locked={locked}>
        {advanced}
      </TeamPanel>
      <div className="fe-add" role="toolbar" aria-label="Add to the team">
        {OFFERED_KINDS.map((k) => (
          <Btn key={k} className={kindVendor(k).cls} disabled={locked} aria-describedby={locked ? `${ids}-lock` : undefined} aria-label={`Add a ${kindVendor(k).name} agent`} onClick={() => onAdd(k)}>
            <VendorMark v={kindVendor(k)} size="xs" /> {kindVendor(k).name}
            <Icon name="plus" size={13} className="muted" />
          </Btn>
        ))}
        {locked && (
          <span id={`${ids}-lock`} className="reason">
            {LOCK_REASON}
          </span>
        )}
      </div>
      {draft.agents.length === 0 && <p className="muted pad">No agents yet. Add one above.</p>}
      {draft.agents.map((a) => {
        const n = errorsOf(issues.filter((i) => i.element.kind === 'agent' && i.element.id === a.id)).length;
        const v = kindVendor(a.kind);
        return (
          <details key={a.id} className="fe-agent" open={draft.agents.length <= 2 || n > 0}>
            <summary>
              <span className={v.cls}>
                <VendorMark v={v} size="xs" />{' '}
              </span>
              <span className="fe-name">{a.name.trim() || 'Unnamed agent'}</span>
              <span className={n ? 'tone-red' : 'tone-green'}>{n ? ` ${MARK.error} ${n} to fix` : ' ✓ ready'}</span>
            </summary>
            <AgentPanel bare draft={draft} agentId={a.id} issues={issues} edit={edit} locked={locked} onSelect={() => {}} />
          </details>
        );
      })}
    </div>
  );
}

/**
 * The Structure board (Edit): design the team, then Start it. Edits autosave; Start saves first,
 * starts, and switches to the Live board. While the team runs, structure is read-only.
 */
export function StructureBoard({ teamId, running, started, narrow, detailsShown = true, onShowDetails, initialAgent, onDraft, onLoadError, onStarted, ref }: Props) {
  const ids = useId();
  const t = useTeamDraft(teamId);
  const [sel, setSel] = useState<Selection | null>(null);
  const [sheet, setSheet] = useState<string | null>(null);
  const [workflowTarget, setWorkflowTarget] = useState<WorkflowTarget | 'list' | null>(null);
  const [starting, setStarting] = useState(false);
  const [statusOpen,setStatusOpen]=useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  // The server held the start because a plan the team uses is near its limit.
  const [planHeld, setPlanHeld] = useState(false);
  const [fixOpen, setFixOpen] = useState(true);
  const startOnce = useRequestOnce();
  const canvas = useRef<CanvasHandle | null>(null);
  const nameRef = useRef<HTMLInputElement | null>(null);
  const focusName = useRef(false);
  const a = useDesignAssistant(teamId);
  const comparison = useMemo(() => (t.draft && a.proposal ? compareDesigns(t.draft, a.proposal.draft) : null), [t.draft, a.proposal]);
  // While the design assistant's proposal is open the board shows it, read-only; the saved design is untouched.
  const draft = comparison?.shown ?? t.draft;
  const edit: Edit = comparison ? () => null : t.edit;
  const proposing = useRef(false);
  proposing.current = comparison !== null;
  const selection = draft ? selectionExists(draft, sel) : null;
  const runLocked = running !== null ? running : t.serverLive;
  const locked = runLocked || comparison !== null;
  const issues = useMemo(() => (a.proposal ? a.proposal.issues : draft ? liveIssues(draft, t.issues) : []), [a.proposal, draft, t.issues]);
  const errors = errorsOf(issues);

  useEffect(() => {
    if (t.draft) onDraft?.(t.draft);
  }, [t.draft, onDraft]);
  useEffect(() => {
    onLoadError?.(t.loadError);
  }, [t.loadError, onLoadError]);

  const inspectSelection = (s: Selection | null) => {
    setSel(s);
    if (s) onShowDetails?.();
  };
  const loaded = draft !== null;
  useEffect(() => {
    if (!loaded || !initialAgent || !t.draft?.agents.some((a) => a.id === initialAgent)) return;
    const s: Selection = { kind: 'agent', id: initialAgent };
    inspectSelection(s);
    canvas.current?.show(s);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialAgent, loaded]);

  // Running state changed (started or stopped elsewhere): re-read issues and the server's view.
  const lastRunning = useRef(running);
  useEffect(() => {
    if (lastRunning.current !== running && running !== null) void t.refresh();
    lastRunning.current = running;
  }, [running, t]);

  useEffect(() => {
    if (focusName.current && selection?.kind === 'agent') {
      focusName.current = false;
      nameRef.current?.focus();
      nameRef.current?.select();
    }
  });

  const addKind = (k: AgentKind) => {
    let id = '';
    t.edit((d) => {
      const r = addAgent(d, k, nextFreeSlot(d));
      id = r.id;
      return r.draft;
    });
    if (id) inspectSelection({ kind: 'agent', id });
  };

  useImperativeHandle(
    ref,
    () => ({
      fit: () => canvas.current?.fit(),
      zoomBy: (f: number) => canvas.current?.zoomBy(f),
      addKey: () => {
        if (narrow) addKind('claude');
        else canvas.current?.focusPalette();
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [narrow],
  );

  // Undo / redo, unless typing (a text field keeps its own undo).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || isTextField(e.target) || proposing.current) return;
      const k = e.key.toLowerCase();
      if (k === 'z' && !e.shiftKey) {
        e.preventDefault();
        t.undo();
      } else if ((k === 'z' && e.shiftKey) || k === 'y') {
        e.preventDefault();
        t.redo();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [t]);

  const reason = comparison ? 'Approve or reject the design assistant\'s proposal first' : draft ? startReason({ issues, saveState: t.saveState, running: runLocked, starting, empty: draft.agents.length === 0 }) : 'Loading…';

  const home = useHome();
  const confirmStart = useStartConfirm();
  const lastResume = useRef(false);
  const start = async (overPlanLimit = false, resume = lastResume.current) => {
    if (reason) return;
    lastResume.current = resume;
    setStarting(true);
    setStartError(null);
    setPlanHeld(false);
    const saved = await t.flush();
    if (!saved.ok) {
      setStarting(false);
      return;
    }
    try {
      const r = overPlanLimit
        ? await api.startTeam(teamId, saved.revision, undefined, true, resume)
        : await startOnce((id) => api.startTeam(teamId, saved.revision, id, false, resume));
      onStarted(r.warnings ?? []);
    } catch (e) {
      if (e instanceof ApiError) {
        if (e.issues) t.setIssues(e.issues);
        if (e.code === 'team_running') void t.refresh();
        if (e.code === 'plan_limit') setPlanHeld(true);
      }
      setStartError(errorText(e));
    } finally {
      setStarting(false);
    }
  };

  const show = (i: DraftIssue) => {
    if (!draft) return;
    const s = selectionFor(draft, i);
    inspectSelection(s);
    if (s) canvas.current?.show(s);
  };

  if (t.loadError)
    return (
      <section className="panel structure" aria-label="Structure">
        <DismissibleError className="pad tone-red">
          Could not load this team: {t.loadError}
        </DismissibleError>
      </section>
    );
  if (!draft)
    return (
      <section className="panel structure" aria-label="Structure">
        <p role="status" className="pad muted">
          Loading team…
        </p>
      </section>
    );

  const advanced = <TeamAdvanced teamId={teamId} draft={draft} flush={t.flush} edit={edit} locked={locked} />;
  const panel =
    selection?.kind === 'agent' && selection.id !== sheet ? (
      <AgentPanel key={selection.id} draft={draft} agentId={selection.id} issues={issues} edit={edit} locked={locked} onSelect={inspectSelection} nameRef={nameRef} />
    ) : selection?.kind === 'group' ? (
      <GroupPanel key={selection.id} draft={draft} groupId={selection.id} issues={issues} edit={edit} locked={locked} onSelect={inspectSelection} />
    ) : selection?.kind === 'connection' ? (
      <ConnectionPanel key={selection.id} draft={draft} connectionId={selection.id} issues={issues} edit={edit} locked={locked} onSelect={inspectSelection} />
    ) : (
      <TeamPanel draft={draft} edit={edit} locked={locked} origin={t.origin} notRunYet={t.startedRevision === null}>
        {advanced}
      </TeamPanel>
    );

  const approve = () => {
    const p = a.proposal;
    if (!p) return;
    t.edit((d) => ({ ...p.draft, id: d.id, revision: d.revision }), 'design-assistant');
    a.end('approved');
  };
  const assistant = (
    <DesignAssistant a={a} revision={t.draft?.revision ?? 0} lockedReason={runLocked ? 'Stop the team to change its design.' : null} comparison={comparison} onApprove={approve}
      onInspect={(target) => { inspectSelection(target); canvas.current?.show(target); }} />
  );

  const fixes = issues.length > 0 && (
    <section className={`panel fix-box${fixOpen ? '' : ' closed'}`} aria-label="Things to fix">
      <div className="panel-bar">
        <span className="panel-title tone-red">
          <Icon name="alert" size={15} />
          Things to fix <Badge tone="red">{issues.length}</Badge>
        </span>
        <span className="panel-rule" aria-hidden="true" />
        <Btn variant="ghost" size="sm" iconOnly icon={fixOpen ? 'chevron-down' : 'chevron-right'} onClick={() => setFixOpen((o) => !o)} aria-expanded={fixOpen} aria-label={fixOpen ? 'Hide things to fix' : 'Show things to fix'} />
      </div>
      {fixOpen && (
        <ul className="fix-list">
          {issues.map((i, n) => {
            const can = selectionFor(draft, i);
            const text = (
              <>
                <span className={i.severity === 'error' ? 'tone-red' : 'tone-amber'} aria-hidden="true">
                  {MARK[i.severity]}{' '}
                </span>
                {i.message}
              </>
            );
            return (
              <li key={`${i.element.kind}-${i.element.id}-${i.code}-${n}`}>
                {can ? (
                  <button type="button" className="fix-item" onClick={() => show(i)}>
                    {text}
                  </button>
                ) : (
                  <span className="fix-item">{text}</span>
                )}
                {i.detail && (
                  <details className="fix-detail" data-advanced>
                    <summary>Advanced</summary>
                    <code>{i.spec_path}</code> {i.detail}
                  </details>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );

  return (
    <section className={`panel structure${narrow ? ' narrow-structure' : ''}`} aria-label={`Structure of ${draft.name || 'this team'}`}>
      <StartRefused refusal={startError ? { text: startError, code: planHeld ? 'plan_limit' : undefined } : null}
        onClose={() => { setStartError(null); setPlanHeld(false); }} onAnyway={() => void start(true)} />
      <div className="panel-bar">
        <span className="panel-title">
          <ModeTabs teamId={teamId} mode="structure" started={started} name={draft.name || 'Unnamed team'} title={<RenameProject draft={draft} edit={edit} />} />
        </span>
        <span className="panel-rule" aria-hidden="true" />
        <span className="panel-tools">
          {draft.workflow && <Btn onClick={() => setWorkflowTarget('list')}>Workflow list</Btn>}
          <Btn icon="bot" aria-pressed={a.open} onClick={() => { a.setOpen(!a.open || !!a.proposal); onShowDetails?.(); }}>Design assistant</Btn>
          <SaveStatus state={t.saveState} error={t.saveError} onRetry={() => void t.retry()} onReload={() => void t.reload()} onKeepMine={() => void t.keepMine()} />
          <details className="structure-status" open={statusOpen} onToggle={e=>setStatusOpen(e.currentTarget.open)} onKeyDown={e=>{if(e.key==='Escape')setStatusOpen(false);}}>
            <summary className={runLocked ? 'st-running' : reason ? 'st-setup' : 'st-ready'}>{runLocked ? 'Running' : reason ? 'Needs setup' : 'Ready'}</summary>
          <div className="sb-status"><header className="setup-notice-head"><strong>{errors.length?`${errors.length} things to fix`:runLocked?'Project running':'Project status'}</strong><button className="btn" aria-label="Dismiss setup notice" onClick={()=>setStatusOpen(false)}>×</button></header>
            {runLocked ? (
              <p id={`${ids}-why`} className="run-banner tone-amber" role="status">
                <span aria-hidden="true">◑ </span>This team is running. You can rename agents, edit their jobs and move cards. To add, remove or reconnect
                agents, stop the team first{started ? ' on the ' : '.'}
                {started && <Link to={teamPath(teamId)}>Run board</Link>}
                {started ? '.' : ''}
              </p>
            ) : null}
            {reason && !runLocked && (
              <div className={`start-why ${errors.length ? 'tone-red' : 'muted'}`}>
                <span id={`${ids}-why`} className="sr-only">Start team: {reason}</span>
                {errors.length?<ul>{errors.map((issue,i)=><li key={i}>{issue.message}</li>)}</ul>:<p>{reason}</p>}
              </div>
            )}
            {!reason && !runLocked && (
              <p className="start-why tone-green">
                <span aria-hidden="true">✓ </span>{draft.workflow ? 'Ready to start. The dispatcher reads Input first; workers wait for assignments.' : 'Ready to start. Each agent gets its job as its first message.'}
              </p>
            )}
            {t.notice && (
              <p className="tone-amber notice" role="status">
                {t.notice}
              </p>
            )}
          </div>
          </details>
          <Btn
            variant="primary"
            icon="play"
            className="start-btn"
            // Real agents use your own logins: say what starts, where, before it does.
            onClick={() => confirmStart.ask(startConfirmation(draft, home), (c) => void start(false, c.resume))}
            disabled={!!reason} aria-describedby={reason ? `${ids}-why` : undefined} aria-label="Start team">
            <span>
              Start<span className="wide-only"> team</span>
            </span>
          </Btn>
        </span>
      </div>

      {narrow && !draft.workflow ? (
        <div className="sb-narrow">
          {fixes}
          {a.open && assistant}
          <FormEditor draft={draft} issues={issues} edit={edit} locked={locked} onAdd={addKind} advanced={advanced} />
        </div>
      ) : (
        <div className={`sb-body${detailsShown ? '' : ' inspector-hidden'}`}>
          <div className="sb-stage">
            {a.busy ? <WorkingBar a={a} /> : comparison && <ProposalBar a={a} comparison={comparison} onApprove={approve} />}
            <StructureCanvas
              ref={canvas}
              teamId={teamId}
              draft={draft}
              issues={issues}
              selection={selection}
              onSelect={setSel}
              onInspect={inspectSelection}
              edit={edit}
              locked={locked}
              onOpen={setSheet}
              onWorkflow={setWorkflowTarget}
              onOpenAgent={(id) => {
                onShowDetails?.();
                focusName.current = true;
                setSel({ kind: 'agent', id });
              }}
              canUndo={t.canUndo && !comparison}
              canRedo={t.canRedo && !comparison}
              marks={comparison?.marks}
              onUndo={t.undo}
              onRedo={t.redo}
            />
            {fixes}
          </div>
          <div id="workspace-details" className="sb-side" hidden={!detailsShown}>{a.open ? <>{assistant}{selection && panel}</> : panel}</div>
        </div>
      )}
      {draft.workflow && workflowTarget && <WorkflowInspector draft={draft} target={workflowTarget} locked={locked} edit={fn => edit(fn)} onOpen={setSheet} onClose={() => setWorkflowTarget(null)} />}
      {sheet && draft && <AgentSheet draft={draft} agentId={sheet} issues={issues} edit={edit} locked={locked} onSelect={inspectSelection} onClose={() => setSheet(null)} />}
      {confirmStart.dialog}
    </section>
  );
}
