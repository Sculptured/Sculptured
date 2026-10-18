import './input.css';
import { useEffect, useRef } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import { connectionPath, type Box } from '../draft/geometry';
import { linkInput, removeInput, setInputText, teamInputBox } from '../draft/input';
import { Btn } from '../shell/ui';
import { InputCardFace, InputMaterial } from './InputMaterial';
import { LOCK_REASON, nameOf, type Edit } from './Panels';

/** The wire's start while dragging from the Input's ● (it is not an agent). */
export const INPUT_WIRE = '$team-input';

type NodeProps = {
  draft: TeamDraft;
  box: Box;
  selected: boolean;
  locked: boolean;
  controls: Omit<React.ComponentProps<'button'>, 'className' | 'style'>;
  onPortDown: (e: React.PointerEvent) => void;
};

/** The Input box: its first lines, who gets it, and a ● to drag to another agent. */
export function InputNode({ draft, box, selected, locked, controls, onPortDown }: NodeProps) {
  const input = draft.input!;
  const names = input.to.map((id) => nameOf(draft, id));
  return (
    <div className="team-input-slot" style={{ left: box.x, top: box.y, width: box.w, height: box.h }}>
      <button type="button" {...controls} className={`team-input-node input-card is-compact${input.text.trim() ? '' : ' is-empty'}${selected ? ' selected' : ''}`} style={{ inset: 0 }}
        aria-label={`Input: ${input.text.trim() ? input.text.trim().slice(0, 80) : 'empty'}. Goes to ${names.join(', ') || 'no agent'}`}>
        <InputCardFace text={input.text} room={1} action={locked ? 'Open' : input.text.trim() ? 'Edit' : 'Add'}
          foot={names.length ? `→ ${names.join(', ')}` : 'Drag ● to an agent'} />
      </button>
      {!locked && <span className="port out" role="presentation" data-tip="Drag to an agent to give it the Input" onPointerDown={onPortDown} />}
    </div>
  );
}

/** Dotted arrows from the Input to each agent it goes to (agents hidden in a closed group are skipped). */
export function InputEdges({ draft, boxes, marker }: { draft: TeamDraft; boxes: Record<string, Box>; marker: string }) {
  const from = teamInputBox(draft);
  if (!from) return null;
  return draft.input!.to.filter((id) => boxes[id]).map((id) => (
    <path key={`input-${id}`} className="edge enforced input-edge" d={connectionPath(from, boxes[id]).d} markerEnd={`url(#${marker})`} />
  ));
}

/** Writing the Input, and choosing who gets it. */
export function InputEditor({ draft, edit, locked, onClose }: { draft: TeamDraft; edit: Edit; locked: boolean; onClose: () => void }) {
  const box = useRef<HTMLElement>(null);
  useEffect(() => box.current?.querySelector<HTMLElement>('input, textarea')?.focus(), []);
  const input = draft.input;
  if (!input) return null;
  return (
    <aside ref={box} className="workflow-inspector team-input-editor" role="dialog" aria-label="Input" tabIndex={-1} onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
      <header><h2>Input</h2><Btn variant="ghost" icon="x" iconOnly onClick={onClose} aria-label="Close Input" kbd="esc" /></header>
      <div className="workflow-inspector-body">
        <p className="muted">The goal, links, notes and files the team starts from. Each agent it goes to gets it after its Job when the team starts.</p>
        <InputMaterial text={input.text} onChange={(text) => edit((d) => setInputText(d, text), 'input-text')} locked={false} lockedReason={LOCK_REASON} rawLabel="Input text" folder={draft.folder ?? null} />
        <fieldset disabled={locked} title={locked ? LOCK_REASON : undefined}>
          <legend>Goes to</legend>
          {draft.agents.length === 0 && <p className="muted">Add an agent first.</p>}
          {draft.agents.map((a) => (
            <label key={a.id}><input type="checkbox" checked={input.to.includes(a.id)} onChange={(e) => edit((d) => linkInput(d, a.id, e.target.checked))} />{a.name.trim() || 'Unnamed agent'}</label>
          ))}
        </fieldset>
        <Btn disabled={locked} tip={locked ? LOCK_REASON : undefined} onClick={() => { edit(removeInput); onClose(); }}>Remove the Input</Btn>
      </div>
    </aside>
  );
}
