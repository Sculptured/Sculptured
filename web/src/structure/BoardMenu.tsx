import { useEffect, useRef, type ReactNode } from 'react';
import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import { copyOf, pasteClip, readClip, writeClip, type PasteAt } from '../draft/clipboard';
import { removeElement, takeOut, ungroup, type Selection } from '../draft/edits';
import { drawnGroups, looseAllowed, type Point } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { inputAllowed, removeInput } from '../draft/input';
import { setKind } from '../draft/models';
import { isTextField } from '../dom';
import { VendorMark } from '../shell/ui';
import { KIND_LABEL } from '../vocab';
import { OFFERED_KINDS } from './ModelControls';
import { kindVendor, LOCK_REASON, type Edit } from './Panels';

/** Closes on Escape and on a pointer press outside `ref`. */
export function useDismiss(ref: React.RefObject<HTMLElement | null>, onClose: () => void) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const inner = (document.activeElement as HTMLElement | null)?.closest('[role="dialog"]');
      if (e.key !== 'Escape' || (inner && inner !== ref.current)) return;
      e.stopPropagation();
      onClose();
    };
    const down = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    window.addEventListener('keydown', key, true);
    window.addEventListener('pointerdown', down, true);
    return () => {
      window.removeEventListener('keydown', key, true);
      window.removeEventListener('pointerdown', down, true);
    };
  }, [ref, onClose]);
}

function MenuItem({ children, onClick, disabled, checked, why = LOCK_REASON }: { children: ReactNode; onClick: () => void; disabled?: boolean; checked?: boolean; why?: string }) {
  return (
    <li role="none">
      <button type="button" role={checked === undefined ? 'menuitem' : 'menuitemradio'} aria-checked={checked} disabled={disabled} title={disabled ? why : undefined} onClick={onClick}>
        {children}
      </button>
    </li>
  );
}

/** What was right-clicked: empty board (at a board point), a group, or an agent card. */
export type MenuTarget = { kind: 'board'; at: Point } | { kind: 'group'; id: string; at: Point } | { kind: 'agent'; id: string } | { kind: 'input' };
export type MenuOpen = MenuTarget & { x: number; y: number };

export type BoardActions = {
  add: (k: AgentKind, at: Point) => void;
  addGroup: (at: Point) => void;
  copy: (s: Selection) => void;
  paste: (where: PasteAt) => void;
  duplicate: (s: Selection) => void;
  inspect: (s: Selection) => void;
  openAgent: (id: string) => void;
  addInput: (at: Point) => void;
  openInput: () => void;
  fit: () => void;
};

type Ctx = { draft: TeamDraft; locked: boolean; groupOnly?: boolean; edit: Edit; act: BoardActions; run: (fn: () => void) => () => void };

const Sep = () => <li role="separator" />;

function AddItems({ at, c }: { at: Point; c: Ctx }) {
  return OFFERED_KINDS.map((k) => (
    <MenuItem key={k} disabled={c.locked} onClick={c.run(() => c.act.add(k, at))}>
      <VendorMark v={kindVendor(k)} size="xs" /> Add a {KIND_LABEL[k]} agent here
    </MenuItem>
  ));
}

function PasteItem({ where, c, label = 'Paste here' }: { where: PasteAt; c: Ctx; label?: string }) {
  const clip = readClip();
  if (!clip) return null;
  const what = clip.kind === 'agent' ? clip.agent.name || 'agent' : clip.block.name || 'group';
  return <MenuItem disabled={c.locked} onClick={c.run(() => c.act.paste(where))}>{label}: {what} <span className="muted">· Ctrl+V</span></MenuItem>;
}

function BoardItems({ at, c }: { at: Point; c: Ctx }) {
  return (
    <>
      <AddItems at={at} c={c} />
      {!c.groupOnly && <MenuItem disabled={c.locked} onClick={c.run(() => c.act.addGroup(at))}>New group here</MenuItem>}
      {!c.groupOnly && inputAllowed(c.draft) && !c.draft.input && <MenuItem disabled={c.locked} onClick={c.run(() => c.act.addInput(at))}>Add the Input here</MenuItem>}
      <PasteItem where={{ at }} c={c} />
      <Sep />
      <MenuItem onClick={c.run(c.act.fit)}>Fit the board to the view <span className="muted">· f</span></MenuItem>
    </>
  );
}

function GroupItems({ id, at, c }: { id: string; at: Point; c: Ctx }) {
  const s: Selection = { kind: 'group', id };
  const clip = readClip();
  return (
    <>
      <MenuItem onClick={c.run(() => c.act.inspect(s))}>Rename and settings</MenuItem>
      <MenuItem onClick={c.run(() => c.act.copy(s))}>Copy group <span className="muted">· Ctrl+C</span></MenuItem>
      {!c.groupOnly && <MenuItem disabled={c.locked} onClick={c.run(() => c.act.duplicate(s))}>Duplicate group <span className="muted">· Ctrl+D</span></MenuItem>}
      <PasteItem where={clip?.kind === 'agent' ? { into: id } : {}} c={c} label={clip?.kind === 'agent' ? 'Paste into this group' : 'Paste'} />
      <Sep />
      <AddItems at={at} c={c} />
      {!c.groupOnly && (
        <>
          <Sep />
          <MenuItem disabled={c.locked} onClick={c.run(() => c.edit((d) => ungroup(d, id)))}>Ungroup <span className="muted">· keeps its agents</span></MenuItem>
          <MenuItem disabled={c.locked} onClick={c.run(() => c.edit((d) => removeElement(d, s)))}>
            <span className="tone-red">Delete group and its agents</span>
          </MenuItem>
        </>
      )}
    </>
  );
}

function AgentItems({ id, c }: { id: string; c: Ctx }) {
  const a = c.draft.agents.find((q) => q.id === id)!;
  const s: Selection = { kind: 'agent', id };
  const grouped = a.group_id !== null && drawnGroups(c.draft).some((g) => g.id === groupOf(a)) && looseAllowed(c.draft);
  const kinds: AgentKind[] = a.kind === 'practice' ? [...OFFERED_KINDS, 'practice'] : OFFERED_KINDS;
  return (
    <>
      <MenuItem onClick={c.run(() => c.act.openAgent(id))}>Open details</MenuItem>
      <MenuItem onClick={c.run(() => c.act.copy(s))}>Copy <span className="muted">· Ctrl+C</span></MenuItem>
      <MenuItem disabled={c.locked} onClick={c.run(() => c.act.duplicate(s))}>Duplicate <span className="muted">· Ctrl+D</span></MenuItem>
      {grouped && !c.groupOnly && (
        <MenuItem disabled={c.locked} onClick={c.run(() => c.edit((d) => takeOut(d, id)))}>
          Take out of its group
        </MenuItem>
      )}
      <Sep />
      {kinds.map((k) => (
        <MenuItem key={k} checked={a.kind === k} disabled={c.locked} onClick={c.run(() => c.edit((d) => setKind(d, id, k)))}>
          <VendorMark v={kindVendor(k)} size="xs" /> Runs on {KIND_LABEL[k]}
        </MenuItem>
      ))}
      <Sep />
      <MenuItem disabled={c.locked} onClick={c.run(() => c.edit((d) => removeElement(d, s)))}>
        <span className="tone-red">Delete agent</span> <span className="muted">· Ctrl+Z undoes</span>
      </MenuItem>
    </>
  );
}

function InputItems({ c }: { c: Ctx }) {
  return (
    <>
      <MenuItem onClick={c.run(c.act.openInput)}>Edit the Input</MenuItem>
      <MenuItem disabled={c.locked} onClick={c.run(() => c.edit(removeInput))}><span className="tone-red">Remove the Input</span></MenuItem>
    </>
  );
}

const menuName = (d: TeamDraft, t: MenuTarget) =>
  t.kind === 'input' ? 'Input actions' : t.kind === 'agent' ? `${d.agents.find((a) => a.id === t.id)?.name || 'Agent'} actions` : t.kind === 'group' ? `${d.groups.find((g) => g.id === t.id)?.label || 'Group'} actions` : 'Board actions';

/** The right-click menu on the Design board, a group or an agent card. */
export function BoardMenu({ open, onClose, ...rest }: { open: MenuOpen; draft: TeamDraft; locked: boolean; groupOnly?: boolean; edit: Edit; act: BoardActions; onClose: () => void }) {
  const box = useRef<HTMLUListElement>(null);
  useDismiss(box, onClose);
  useEffect(() => box.current?.querySelector('button')?.focus(), []);
  const gone = open.kind === 'agent' ? !rest.draft.agents.some((a) => a.id === open.id) : open.kind === 'group' ? !rest.draft.groups.some((g) => g.id === open.id) : open.kind === 'input' && !rest.draft.input;
  if (gone) return null;
  const c: Ctx = { ...rest, run: (fn) => () => { fn(); onClose(); } };
  return (
    <ul ref={box} className="ctx-menu" role="menu" aria-label={menuName(rest.draft, open)} style={{ left: Math.min(open.x, window.innerWidth - 250), top: Math.min(open.y, window.innerHeight - 340) }}>
      {open.kind === 'board' && <BoardItems at={open.at} c={c} />}
      {open.kind === 'group' && <GroupItems id={open.id} at={open.at} c={c} />}
      {open.kind === 'agent' && <AgentItems id={open.id} c={c} />}
      {open.kind === 'input' && <InputItems c={c} />}
    </ul>
  );
}

type ClipOpts = { draft: TeamDraft; selection: Selection | null; edit: Edit; locked: boolean; groupOnly?: boolean; onPasted: (s: Selection) => void };

/** Copy, paste and duplicate on the board, by menu or by Ctrl/⌘+C, V and D (never inside a text field). */
export function useBoardClipboard({ draft, selection, edit, locked, groupOnly, onPasted }: ClipOpts) {
  const paste = (where: PasteAt, clip = readClip()) => {
    if (!clip) return;
    if (locked) return void edit((d) => ({ draft: d, reason: LOCK_REASON }));
    if (groupOnly && clip.kind === 'group') return void edit((d) => ({ draft: d, reason: 'A saved group holds one group. Paste a group on a project board.' }));
    const into = groupOnly ? draft.groups[0]?.id : where.into;
    let made: Selection | undefined;
    const reason = edit((d) => {
      const r = pasteClip(d, clip, clip.kind === 'agent' ? { ...where, into } : { at: where.at });
      made = r.select;
      return r;
    });
    if (!reason && made) onPasted(made);
  };
  const copy = (s: Selection) => {
    const clip = copyOf(draft, s);
    if (clip) writeClip(clip);
    return clip;
  };
  const duplicate = (s: Selection) => {
    const clip = copyOf(draft, s);
    if (clip) paste({}, clip);
  };
  const latest = useRef({ copy, paste, duplicate, selection });
  latest.current = { copy, paste, duplicate, selection };
  useClipboardKeys(latest);
  return { copy, paste, duplicate };
}

type ClipKeys = { copy: (s: Selection) => unknown; paste: (where: PasteAt) => void; duplicate: (s: Selection) => void; selection: Selection | null };

const ctrlOnly = (e: KeyboardEvent) => (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey;

/** Whether this keypress may be a board shortcut: not in a text field or an open dialog. */
const boardKeyFree = (e: KeyboardEvent) => ctrlOnly(e) && !isTextField(e.target) && !e.defaultPrevented && !document.querySelector('[role="dialog"][aria-modal="true"]');

type ClipHandler = (e: KeyboardEvent, keys: ClipKeys, item: Exclude<Selection, { kind: 'connection' }> | null) => void;

const CLIP_HANDLERS: Record<string, ClipHandler> = {
  c: (e, keys, item) => { if (item && !window.getSelection()?.toString() && keys.copy(item)) e.preventDefault(); },
  v: (e, keys) => { if (readClip()) { e.preventDefault(); keys.paste(keys.selection?.kind === 'group' ? { into: keys.selection.id } : {}); } },
  d: (e, keys, item) => { if (item) { e.preventDefault(); keys.duplicate(item); } },
};

function clipboardKey(e: KeyboardEvent, keys: ClipKeys) {
  const s = keys.selection;
  CLIP_HANDLERS[e.key.toLowerCase()]?.(e, keys, s && s.kind !== 'connection' ? s : null);
}

/** Ctrl/⌘+C, V and D on the board, outside text fields and dialogs. Text selected on the page copies as text. */
function useClipboardKeys(latest: React.RefObject<ClipKeys>) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (boardKeyFree(e)) clipboardKey(e, latest.current); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [latest]);
}
