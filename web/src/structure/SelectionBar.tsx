import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode, type RefObject } from 'react';
import { groupAgents, moveAgentsToGroup, removeAgents, setFolderFor, setKindFor, setLimitsFor, setModelFor, type BulkLimits } from '../draft/bulk';
import { drawnGroups, looseAllowed } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { isModelKind } from '../draft/models';
import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import { plural } from '../format';
import { Btn, Icon, VendorMark } from '../shell/ui';
import { KIND_LABEL } from '../vocab';
import { FolderPicker } from './FolderPicker';
import { MODEL_CHOICES, OFFERED_KINDS } from './ModelControls';
import { kindVendor, LOCK_REASON, type Edit } from './Panels';

type Menu = 'model' | 'group' | 'limits' | null;
/** One change to every selected agent, as one edit (one undo step). */
type Apply = (fn: (d: TeamDraft) => TeamDraft) => void;

type Props = {
  draft: TeamDraft;
  ids: string[];
  edit: Edit;
  locked: boolean;
  onClear: () => void;
  /** A new group was made for them. */
  onGrouped?: (groupId: string) => void;
};

/**
 * Shown while two or more agents are selected on the Design board: change them all at once.
 * Every change is one edit, so one Ctrl+Z undoes it. While the team runs only moving is allowed;
 * the bar says so.
 */
export function SelectionBar({ draft, ids, edit, locked, onClear, onGrouped }: Props) {
  const uid = useId();
  const root = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useMenu(root);
  const agents = draft.agents.filter((a) => ids.includes(a.id));
  const off = locked ? { disabled: true, 'aria-describedby': `${uid}-lock` } : {};
  const apply: Apply = (fn) => {
    edit(fn);
    setMenu(null);
  };
  const pop = (m: Exclude<Menu, null>, label: ReactNode, body: ReactNode, icon?: 'group') => (
    <MenuButton open={menu === m} onToggle={() => setMenu(menu === m ? null : m)} label={label} icon={icon} off={off}>{body}</MenuButton>
  );
  return (
    <div ref={root} className="selection-bar" role="toolbar" aria-label={`${plural(agents.length, 'agent')} selected`} onKeyDown={barKeys(menu, setMenu, onClear)}>
      <span className="sel-count" role="status"><strong>{agents.length}</strong> agents selected</span>
      <span className="sel-sep" aria-hidden="true" />
      {pop('model', <KindLabel draft={draft} ids={ids} />, <ModelMenu draft={draft} ids={ids} apply={apply} />)}
      {pop('group', 'Group', <GroupMenu draft={draft} ids={ids} edit={edit} onDone={(id) => { setMenu(null); if (id) onGrouped?.(id); }} />, 'group')}
      <FolderButton off={off} initial={agents[0]?.folder ?? draft.folder ?? null} onChoose={(folder) => edit((d) => setFolderFor(d, ids, folder))} />
      {pop('limits', 'Limits', <LimitsForm n={agents.length} onApply={(l) => apply((d) => setLimitsFor(d, ids, l))} />)}
      <Btn size="sm" danger icon="trash" {...off} onClick={() => { edit((d) => removeAgents(d, ids)); onClear(); }} tip="Delete them and their arrows. Ctrl+Z brings them back." kbd="del">Delete</Btn>
      <span className="sel-sep" aria-hidden="true" />
      <Btn size="sm" variant="ghost" icon="x" onClick={onClear} kbd="esc" aria-label="Clear the selection"><kbd>Esc</kbd></Btn>
      {locked && <span id={`${uid}-lock`} className="reason sel-reason">{LOCK_REASON} You can still move them.</span>}
    </div>
  );
}

type Off = { disabled?: boolean; 'aria-describedby'?: string };

function MenuButton({ open, onToggle, label, icon, off, children }: { open: boolean; onToggle: () => void; label: ReactNode; icon?: 'group'; off: Off; children: ReactNode }) {
  return (
    <span className="sel-item">
      <Btn size="sm" icon={icon} {...off} aria-expanded={open} onClick={onToggle}>
        {label} <Icon name="chevron-down" size={12} />
      </Btn>
      {open && children}
    </span>
  );
}

/** One folder for all of them (each agent's own folder). */
function FolderButton({ off, initial, onChoose }: { off: Off; initial: string | null; onChoose: (folder: string) => void }) {
  const [picking, setPicking] = useState(false);
  return (
    <>
      <Btn size="sm" icon="folder" {...off} onClick={() => setPicking(true)}>Folder…</Btn>
      {picking && <FolderPicker initialPath={initial} onClose={() => setPicking(false)} onChoose={(folder) => { onChoose(folder); setPicking(false); }} />}
    </>
  );
}

/** The open menu; a press anywhere outside the bar closes it. */
function useMenu(root: RefObject<HTMLDivElement | null>) {
  const [menu, setMenu] = useState<Menu>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) setMenu(null); };
    document.addEventListener('pointerdown', close);
    return () => document.removeEventListener('pointerdown', close);
  }, [menu, root]);
  return [menu, setMenu] as const;
}

/** Esc closes the open menu, then lets go of the selection. */
function barKeys(menu: Menu, setMenu: (m: Menu) => void, onClear: () => void) {
  return (e: React.KeyboardEvent) => {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    if (menu) setMenu(null);
    else onClear();
  };
}

const kindsOf = (draft: TeamDraft, ids: string[]) => [...new Set(draft.agents.filter((a) => ids.includes(a.id)).map((a) => a.kind))];

function KindLabel({ draft, ids }: { draft: TeamDraft; ids: string[] }) {
  const kinds = kindsOf(draft, ids);
  const one = kinds.length === 1 ? kinds[0] : null;
  return <>{one && <VendorMark v={kindVendor(one)} size="xs" />}{one ? KIND_LABEL[one] : 'Mixed'} · model</>;
}

/** Claude or Codex for all of them, then a model when they are all one kind. */
function ModelMenu({ draft, ids, apply }: { draft: TeamDraft; ids: string[]; apply: Apply }) {
  const kinds = kindsOf(draft, ids);
  const one = kinds.length === 1 ? kinds[0] : null;
  return (
    <div className="sel-pop" role="group" aria-label="Claude or Codex, and model">
      <span className="sel-pop-label">Run them with</span>
      <div className="sel-pop-row">
        {OFFERED_KINDS.map((k: AgentKind) => (
          <Btn key={k} size="sm" className={kindVendor(k).cls} pressed={one === k} onClick={() => apply((d) => setKindFor(d, ids, k))}>
            <VendorMark v={kindVendor(k)} size="xs" /> {KIND_LABEL[k]}
          </Btn>
        ))}
      </div>
      <span className="sel-pop-label">Model</span>
      {one && isModelKind(one) ? (
        <div className="sel-pop-col">
          <button type="button" className="sel-opt" onClick={() => apply((d) => setModelFor(d, ids, null))}>Default <span className="muted">· the group's or team's</span></button>
          {MODEL_CHOICES[one].map((m) => (
            <button key={m.id} type="button" className="sel-opt" onClick={() => apply((d) => setModelFor(d, ids, m.id))}>{m.label}</button>
          ))}
        </div>
      ) : (
        <p className="reason">They mix Claude and Codex. Choose one of them above to pick a model.</p>
      )}
    </div>
  );
}

/** Into a drawn group, out of every group, or into a new group of their own. */
function GroupMenu({ draft, ids, edit, onDone }: { draft: TeamDraft; ids: string[]; edit: Edit; onDone: (newGroup?: string) => void }) {
  const groups = drawnGroups(draft);
  const homes = new Set(draft.agents.filter((a) => ids.includes(a.id)).map((a) => groupOf(a)));
  const move = (g: string | null) => { edit((d) => moveAgentsToGroup(d, ids, g)); onDone(); };
  const fresh = () => {
    let id = '';
    edit((d) => {
      const r = groupAgents(d, ids);
      id = r.id;
      return r.draft;
    });
    onDone(id || undefined);
  };
  return (
    <div className="sel-pop" role="group" aria-label="Put them in a group">
      <div className="sel-pop-col">
        {groups.map((g) => (
          <button key={g.id} type="button" className="sel-opt" aria-pressed={homes.size === 1 && homes.has(g.id)} onClick={() => move(g.id)}><Icon name="group" size={12} /> {g.label || 'Unnamed group'}</button>
        ))}
        {groups.length > 0 && looseAllowed(draft) && <button type="button" className="sel-opt" onClick={() => move(null)}>Outside every group</button>}
        <button type="button" className="sel-opt" onClick={fresh}><Icon name="plus" size={12} /> New group with these {ids.length}</button>
      </div>
    </div>
  );
}

const whole = (text: string): number | undefined => {
  const n = Math.floor(Number(text));
  return text.trim() !== '' && Number.isFinite(n) && n > 0 ? n : undefined;
};

/** Hard limits for one run, for all of them; empty boxes leave each agent's own limit as it is. */
function LimitsForm({ n, onApply }: { n: number; onApply: (l: BulkLimits) => void }) {
  const uid = useId();
  const [v, setV] = useState({ tokens: '', turns: '', minutes: '' });
  const l: BulkLimits = { tokens: whole(v.tokens), turns: whole(v.turns), minutes: whole(v.minutes) };
  const empty = l.tokens === undefined && l.turns === undefined && l.minutes === undefined;
  const submit = (e: FormEvent) => { e.preventDefault(); if (!empty) onApply(l); };
  const field = (key: keyof typeof v, label: string, first = false) => (
    <label>{label} <input autoFocus={first} className="fld-input" type="number" min={1} placeholder="no change" value={v[key]} onChange={(e) => setV({ ...v, [key]: e.target.value })} /></label>
  );
  return (
    <form className="sel-pop sel-limits" aria-label="Limits for one run" onSubmit={submit}>
      <span className="sel-pop-label">Limits for one run, for all {n}</span>
      {field('tokens', 'Tokens', true)}
      {field('turns', 'Turns')}
      {field('minutes', 'Minutes working')}
      <p className="muted sel-note">These are hard limits: reaching one stops the agent. Each is asked to wrap up at 85%. Empty boxes stay as they are.</p>
      <div className="sel-pop-row">
        <Btn size="sm" variant="primary" type="submit" disabled={empty} aria-describedby={empty ? `${uid}-why` : undefined}>Apply</Btn>
        <Btn size="sm" onClick={() => onApply({ tokens: null, turns: null, minutes: null })}>Remove all limits</Btn>
        {empty && <span id={`${uid}-why`} className="reason">Type a limit first.</span>}
      </div>
    </form>
  );
}
