import { TextbookSetup } from './TextbookSetup';
import { useEffect, useRef } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Workflow } from '../generated/Workflow';
import { InputMaterial } from '../structure/InputMaterial';
import { WorkflowBoard, WorkflowSettings } from './WorkflowBoard';
import { StageEditor } from './StageEditor';
import { WorkflowRunView } from './WorkflowRunView';
import type { WorkflowTarget } from './WorkflowGraph';
import { addParallelWorker } from './workers';
import { byCommand } from './DispatchCommand';

type Target = WorkflowTarget | 'list';
type Edit = (fn: (d: TeamDraft) => TeamDraft) => void;

const TITLES: Record<string, string> = { input: 'Input', settings: 'Dispatch & composition', run: 'Assembly & work evidence', list: 'Workflow list' };
const titleOf = (target: Target) => (typeof target === 'object' ? 'Stage & branches' : TITLES[target]);

type BodyProps = { draft: TeamDraft; target: Target; locked: boolean; edit: Edit; change: (w: Workflow) => void; onOpen: (id: string) => void };

/** What the inspector shows for its target: Input, dispatch settings, a stage, the run's evidence, or the list. */
function InspectorBody({ draft, target, locked, edit, change, onOpen }: BodyProps) {
  const w = draft.workflow!;
  if (typeof target === 'object') return <StageEditor key={target.stage} draft={draft} stageId={target.stage} locked={locked} change={change} onOpen={onOpen} onAddWorker={() => edit(d => addParallelWorker(d, target.stage))} />;
  switch (target) {
    case 'input': return <WorkflowInputEditor draft={draft} locked={locked} edit={edit} />;
    case 'settings': return <><p>{byCommand(w) ? 'The dispatch command' : 'The dispatcher'} chooses 1–{w.max_tasks} items, each with up to {w.max_steps} steps. Shared agents handle one assignment at a time.</p><WorkflowSettings draft={draft} locked={locked} change={change} editTeam={edit} expanded /></>;
    case 'run': return <><p>Assembly waits for every item to complete its review route. Acceptance is agent-reported; inspect the evidence.</p><WorkflowRunView draft={draft} /></>;
    default: return <WorkflowBoard draft={draft} locked={locked} edit={edit} onOpen={onOpen} />;
  }
}

/** Focus moves into the inspector while it is open and returns to what had it. */
function useInspectorFocus() {
  const box = useRef<HTMLElement>(null), previous = useRef(document.activeElement as HTMLElement | null);
  useEffect(() => { const origin = previous.current; box.current?.focus({ preventScroll: true }); return () => origin?.focus({ preventScroll: true }); }, []);
  return box;
}

type Props = { draft: TeamDraft; target: Target; locked: boolean; edit: Edit; onOpen: (id: string) => void; onClose: () => void; closeLabel?: string; className?: string };

export function WorkflowInspector({ draft, target, locked, edit, onOpen, onClose, closeLabel = 'Close workflow details', className = '' }: Props) {
  const box = useInspectorFocus();
  const change = (workflow: Workflow) => edit(d => ({ ...d, workflow }));
  const title = titleOf(target);
  return <aside className={`workflow-inspector${className ? ` ${className}` : ''}`} ref={box} tabIndex={-1} aria-label={title} onKeyDown={e => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
    <header><h2>{title}</h2><button className="btn" onClick={onClose}>{closeLabel}</button></header>
    <div className="workflow-inspector-body"><InspectorBody draft={draft} target={target} locked={locked} edit={edit} change={change} onOpen={onOpen} /></div>
  </aside>;
}

/** Why the workflow's Input cannot change now. */
export const INPUT_LOCKED = 'Stop the project before changing Input.';

/** The workflow's Input: links, notes and files the dispatcher reads when the team starts. */
export function WorkflowInputEditor({ draft, locked, edit }: { draft: TeamDraft; locked: boolean; edit: (fn: (d: TeamDraft) => TeamDraft) => void }) {
  const w = draft.workflow!;
  return <section className="workflow-input" aria-label="Workflow input editor">
    <InputMaterial text={w.input} onChange={input => edit(d => d.workflow ? { ...d, workflow: { ...d.workflow, input } } : d)} locked={locked} lockedReason={INPUT_LOCKED} rawLabel="Workflow input" folder={draft.folder ?? null} />
    <TextbookSetup draft={draft} locked={locked} edit={edit} />
  </section>;
}
