import { DismissibleError } from '../shell/DismissibleError';
import { useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode, type Ref } from 'react';
import type { AgentKind } from '../generated/AgentKind';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import { selectionExists, type Selection } from '../draft/edits';
import { liveIssues, MARK, selectionFor } from '../draft/issues';
import { Badge, Btn, Icon } from '../shell/ui';
import type { SaveState, TeamDraftApi } from '../useTeamDraft';
import { isTextField } from '../dom';
import { LOCK_REASON, type Edit } from './Panels';
import { StructureCanvas, type CanvasHandle } from './StructureCanvas';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';
import { useDesignAssistant } from './assistant/useDesignAssistant';
import { DesignAssistant, ProposalBar, WorkingBar } from './assistant/DesignAssistant';
import { compareDesigns } from './assistant/compare';
import { ConnectionBar, GroupBar } from './ElementBar';

/** What the command palette reads from the board: null while it loads. */
export type BoardStatus = { canUndo: boolean; canRedo: boolean; startReason: string | null; lockReason: string | null; workflow: boolean };
export type BoardHandle = {
  fit: () => void;
  zoomBy: (f: number) => void;
  addKey: () => void;
  add: (kind: AgentKind) => void;
  addGroup: () => void;
  selectAll: () => void;
  undo: () => void;
  redo: () => void;
  /** Opens the workflow list (teams with a workflow). */
  workflowList: () => void;
  status: () => BoardStatus | null;
};

type Props = {
  teamId: string;
  /** The project's design and its autosaving edits. */
  t: TeamDraftApi;
  /** From the team's live stream: true while any agent may be running; null when there is no stream. */
  running: boolean | null;
  /** A card was opened (clicked without dragging, or Enter): its agent's page. */
  onOpenAgent: (agentId: string) => void;
  /** A workflow part was opened: its own page. */
  onWorkflow: (target: WorkflowTarget | 'list') => void;
  /** Why Start cannot run, for the palette; null when it can. */
  startReason?: string | null;
  /** Tools before the board's own (the Design | Run switch). */
  leadTools?: ReactNode;
  /** Start must wait while a proposal is shown: the reason, or null. */
  onStartBlock?: (why: string | null) => void;
  ref?: Ref<BoardHandle>;
};

/** The selection each team's board had, kept while you open an agent and come back. */
const SELECTIONS = new Map<string, Selection | null>();
/** Tests start each board afresh. */
export const forgetBoardSelections = () => SELECTIONS.clear();

export function SaveStatus({ state, error, onRetry, onReload, onKeepMine }: { state: SaveState; error: string | null; onRetry: () => void; onReload: () => void; onKeepMine: () => void }) {
  // Saved is the normal state: nothing on screen, still announced to screen readers.
  if (state === 'saved')
    return (
      <span className="save-state sr-only" role="status">
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
      <span aria-hidden="true">▲ </span>not saved: {error}{' '}
      {state === 'conflict' ? (
        <><Btn size="sm" icon="refresh" onClick={onReload}>Reload</Btn> <Btn size="sm" onClick={onKeepMine}>Keep mine</Btn></>
      ) : (
        <Btn size="sm" icon="refresh" onClick={onRetry}>Retry</Btn>
      )}
    </DismissibleError>
  );
}

/** Things to fix, floating over the board: each problem selects or opens what it is about. */
function FixBox({ draft, issues, onShow }: { draft: TeamDraft; issues: DraftIssue[]; onShow: (i: DraftIssue) => void }) {
  // It starts open only while the list is short; a long list starts folded so it never covers the board.
  const [choice, setChoice] = useState<boolean | null>(null);
  const open = choice ?? issues.length <= 3;
  if (issues.length === 0) return null;
  return (
    <section className={`panel fix-box${open ? '' : ' closed'}`} aria-label="Things to fix">
      <div className="panel-bar">
        <span className="panel-title tone-red"><Icon name="alert" size={15} />Things to fix <Badge tone="red">{issues.length}</Badge></span>
        <span className="panel-rule" aria-hidden="true" />
        <Btn variant="ghost" size="sm" iconOnly icon={open ? 'chevron-down' : 'chevron-right'} onClick={() => setChoice(!open)} aria-expanded={open} aria-label={open ? 'Hide things to fix' : 'Show things to fix'} />
      </div>
      {open && (
        <ul className="fix-list">
          {issues.map((i, n) => {
            const text = <><span className={i.severity === 'error' ? 'tone-red' : 'tone-amber'} aria-hidden="true">{MARK[i.severity]} </span>{i.message}</>;
            return (
              <li key={`${i.element.kind}-${i.element.id}-${i.code}-${n}`}>
                {selectionFor(draft, i) ? <button type="button" className="fix-item" onClick={() => onShow(i)}>{text}</button> : <span className="fix-item">{text}</span>}
                {i.detail && <details className="fix-detail" data-advanced><summary>Advanced</summary><code>{i.spec_path}</code> {i.detail}</details>}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/** The selection, restored when the board comes back for the same team. */
function useKeptSelection(teamId: string) {
  const [sel, setSelState] = useState<Selection | null>(() => SELECTIONS.get(teamId) ?? null);
  const setSel = (s: Selection | null) => { SELECTIONS.set(teamId, s); setSelState(s); };
  return [sel, setSel] as const;
}

/**
 * The Design board: the canvas and its toolbar, nothing beside it. A card opens its agent's
 * page; a group or an arrow shows a small bar of its settings over the board. Edits autosave.
 * While the team runs, structure is read-only.
 */
export function StructureBoard({ teamId, t, running, onOpenAgent, onWorkflow, startReason = null, leadTools, onStartBlock, ref }: Props) {
  const [sel, setSel] = useKeptSelection(teamId);
  const canvas = useRef<CanvasHandle | null>(null);
  const a = useDesignAssistant(teamId);
  const comparison = useMemo(() => (t.draft && a.proposal ? compareDesigns(t.draft, a.proposal.draft) : null), [t.draft, a.proposal]);
  // While the design assistant's proposal is open the board shows it, read-only; the saved design is untouched.
  const draft = comparison?.shown ?? t.draft;
  const edit: Edit = comparison ? () => null : t.edit;
  const proposing = comparison !== null;
  const selection = draft ? selectionExists(draft, sel) : null;
  const runLocked = running !== null ? running : t.serverLive;
  const locked = runLocked || proposing;
  const issues = useMemo(() => (a.proposal ? a.proposal.issues : draft ? liveIssues(draft, t.issues) : []), [a.proposal, draft, t.issues]);
  useBoardKeys(t, proposing);
  useEffect(() => {
    onStartBlock?.(proposing ? "Approve or reject the design assistant's proposal first." : null);
    return () => onStartBlock?.(null);
  }, [proposing, onStartBlock]);
  useRefreshOnRunChange(running, t);
  useRestoreFocus(canvas, sel, draft !== null);
  useImperativeHandle(ref, () => ({
    fit: () => canvas.current?.fit(),
    zoomBy: (f: number) => canvas.current?.zoomBy(f),
    addKey: () => canvas.current?.focusPalette(),
    add: (k: AgentKind) => canvas.current?.add(k),
    addGroup: () => canvas.current?.addGroup(),
    selectAll: () => canvas.current?.selectAll(),
    undo: () => t.undo(),
    redo: () => t.redo(),
    workflowList: () => onWorkflow('list'),
    status: () => draft && {
      canUndo: t.canUndo && !proposing, canRedo: t.canRedo && !proposing, startReason: proposing ? "Approve or reject the design assistant's proposal first" : startReason,
      lockReason: locked ? (proposing ? "Approve or reject the design assistant's proposal first." : LOCK_REASON) : null, workflow: !!draft.workflow,
    },
  }));

  if (t.loadError) return <section className="panel structure" aria-label="Structure"><DismissibleError className="pad tone-red">Could not load this team: {t.loadError}</DismissibleError></section>;
  if (!draft) return <section className="panel structure" aria-label="Structure"><p role="status" className="pad muted">Loading team…</p></section>;

  const show = (i: DraftIssue) => {
    const s = selectionFor(draft, i);
    if (s?.kind === 'agent') return onOpenAgent(s.id);
    setSel(s);
    if (s) canvas.current?.show(s);
  };
  const approve = () => {
    const p = a.proposal;
    if (!p) return;
    t.edit((d) => ({ ...p.draft, id: d.id, revision: d.revision }), 'design-assistant');
    a.end('approved');
  };
  const tools = (
    <span className="sb-extra-tools">
      {draft.workflow && <Btn variant="ghost" size="sm" icon="list" onClick={() => onWorkflow('list')} tip="Every stage and item of the workflow, as a list">Workflow list</Btn>}
      <Btn variant="ghost" size="sm" icon="bot" aria-pressed={a.open} onClick={() => a.setOpen(!a.open || !!a.proposal)}>Design assistant</Btn>
      <SaveStatus state={t.saveState} error={t.saveError} onRetry={() => void t.retry()} onReload={() => void t.reload()} onKeepMine={() => void t.keepMine()} />
    </span>
  );
  return (
    <section className="panel structure" aria-label={`Structure of ${draft.name || 'this team'}`}>
      {runLocked && !proposing && <p className="muted board-notice" role="status"><span aria-hidden="true">◑ </span>Running: you can rename agents, edit jobs and move cards. Stop the team to change the rest.</p>}
      {t.notice && <p className="tone-amber notice board-notice" role="status">{t.notice}</p>}
      <div className="sb-body">
        <div className="sb-stage">
          {a.busy ? <WorkingBar a={a} /> : comparison && <ProposalBar a={a} comparison={comparison} onApprove={approve} />}
          <StructureCanvas
            ref={canvas} teamId={teamId} draft={draft} issues={issues} selection={selection} onSelect={setSel} onInspect={setSel} edit={edit} locked={locked}
            onOpen={onOpenAgent} onOpenAgent={onOpenAgent} onWorkflow={onWorkflow}
            canUndo={t.canUndo && !proposing} canRedo={t.canRedo && !proposing} marks={comparison?.marks} onUndo={t.undo} onRedo={t.redo}
            leadTools={leadTools} extraTools={tools}
          />
          {selection?.kind === 'group' && <GroupBar key={selection.id} draft={draft} groupId={selection.id} issues={issues} edit={edit} locked={locked} onClose={() => setSel(null)} />}
          {selection?.kind === 'connection' && <ConnectionBar key={selection.id} draft={draft} connectionId={selection.id} edit={edit} locked={locked} onClose={() => setSel(null)} />}
          <FixBox draft={draft} issues={issues} onShow={show} />
          {a.open && (
            <div className="da-window">
              <DesignAssistant a={a} revision={t.draft?.revision ?? 0} lockedReason={runLocked ? 'Stop the team to change its design.' : null} comparison={comparison} onApprove={approve}
                onInspect={(target) => { setSel(target); canvas.current?.show(target); }} />
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

/** Undo / redo, unless typing (a text field keeps its own undo) or a proposal is shown. */
export function useBoardKeys(t: TeamDraftApi, proposing: boolean) {
  const off = useRef(proposing);
  off.current = proposing;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || isTextField(e.target) || off.current) return;
      const k = e.key.toLowerCase();
      if (k === 'z' && !e.shiftKey) { e.preventDefault(); t.undo(); }
      else if ((k === 'z' && e.shiftKey) || k === 'y') { e.preventDefault(); t.redo(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [t]);
}

/** Running state changed (started or stopped elsewhere): re-read issues and the server's view. */
function useRefreshOnRunChange(running: boolean | null, t: TeamDraftApi) {
  const last = useRef(running);
  useEffect(() => {
    if (last.current !== running && running !== null) void t.refresh();
    last.current = running;
  }, [running, t]);
}

/** Coming back to the board: keyboard focus returns to the card that was selected. */
function useRestoreFocus(canvas: React.RefObject<CanvasHandle | null>, sel: Selection | null, loaded: boolean) {
  const done = useRef(false);
  useEffect(() => {
    if (done.current || !loaded) return;
    done.current = true;
    if (sel) canvas.current?.show(sel);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, when the design first draws
  }, [loaded]);
}
