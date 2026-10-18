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

export function WorkflowInspector({ draft, target, locked, edit, onOpen, onClose, closeLabel = 'Close workflow details', className = '' }: { draft: TeamDraft; target: WorkflowTarget | 'list'; locked: boolean; edit: (fn: (d: TeamDraft) => TeamDraft) => void; onOpen: (id: string) => void; onClose: () => void; closeLabel?: string; className?: string }) {
  const box = useRef<HTMLElement>(null), previous = useRef(document.activeElement as HTMLElement | null);
  useEffect(() => { const origin = previous.current; box.current?.focus({ preventScroll:true }); return () => origin?.focus({ preventScroll:true }); }, []);
  const w = draft.workflow!, change = (workflow: Workflow) => edit(d => ({ ...d, workflow }));
  const title = typeof target === 'object' ? 'Stage & branches' : target === 'input' ? 'Input' : target === 'settings' ? 'Dispatch & composition' : target === 'run' ? 'Assembly & work evidence' : 'Workflow list';
  return <aside className={`workflow-inspector${className ? ` ${className}` : ''}`} ref={box} tabIndex={-1} aria-label={title} onKeyDown={e => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
    <header><h2>{title}</h2><button className="btn" onClick={onClose}>{closeLabel}</button></header>
    <div className="workflow-inspector-body">
      {target === 'input' && <WorkflowInputEditor draft={draft} locked={locked} edit={edit} />}
      {target === 'settings' && <><p>The dispatcher chooses 1–{w.max_tasks} items, each with up to {w.max_steps} steps. Shared agents handle one assignment at a time.</p><WorkflowSettings draft={draft} locked={locked} change={change} editTeam={edit} expanded /></>}
      {typeof target === 'object' && <StageEditor key={target.stage} draft={draft} stageId={target.stage} locked={locked} change={change} onOpen={onOpen} onAddWorker={() => edit(d => addParallelWorker(d,target.stage))} />}
      {target === 'run' && <><p>Assembly waits for every item to complete its review route. Acceptance is agent-reported; inspect the evidence.</p><WorkflowRunView draft={draft} /></>}
      {target === 'list' && <WorkflowBoard draft={draft} locked={locked} edit={edit} onOpen={onOpen} />}
    </div>
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
