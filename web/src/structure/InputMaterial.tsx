import './input.css';
import { useId, useMemo, useState, type ReactNode } from 'react';
import { Btn, Icon, type IconName } from '../shell/ui';
import { TextArea } from '../shell/TextArea';
import { FolderPicker } from './FolderPicker';
import { withFiles } from './PickFiles';
import {
  addInputLink,
  addInputNote,
  countsText,
  fileName,
  inputCounts,
  inputItems,
  normalizeLink,
  removeInputItem,
  shortLink,
  type InputItem,
  type InputItemKind,
} from './inputItems';

const KIND_ICON: Record<InputItemKind, IconName> = { link: 'link', note: 'pencil', file: 'file-text' };

/** An item in one short line, for the board card and the Remove button's name. */
function preview(it: InputItem): string {
  if (it.kind === 'link') return it.text === it.target ? shortLink(it.target!) : it.text.replace(it.target!, shortLink(it.target!));
  if (it.kind === 'file') return fileName(it.target!);
  return it.text.split('\n')[0];
}

/** What the card shows under its title: counts and the first items, or an invitation. */
function CardBody({ items, room }: { items: InputItem[]; room: number }) {
  if (items.length === 0)
    return (
      <span className="input-card-empty">
        <Icon name="plus" size={14} />
        <span>Nothing yet. Add links, notes or files</span>
      </span>
    );
  const shown = items.slice(0, room);
  return (
    <span className="input-card-body">
      <span className="input-card-counts">{countsText(inputCounts(items))}</span>
      {shown.map((it, i) => (
        <span key={i} className={`input-card-item is-${it.kind}`}>
          <Icon name={KIND_ICON[it.kind]} size={12} />
          <span className="input-card-item-text">{preview(it)}</span>
        </span>
      ))}
      {items.length > shown.length && <span className="input-card-more">+{items.length - shown.length} more</span>}
    </span>
  );
}

/**
 * The Input on the board: a clear title, what goes in, and what has been added (counts and the
 * first items), or an inviting empty state. Only spans: it is drawn inside the node's button.
 * `room`: how many items fit under the counts. `action`: the open affordance's words. `foot`: who gets it.
 */
export function InputCardFace({ text, room, action, foot }: { text: string; room: number; action: string; foot?: ReactNode }) {
  const items = useMemo(() => inputItems(text), [text]);
  return (
    <>
      <span className="input-card-head">
        <span className="input-card-icon" aria-hidden="true"><Icon name="inbox" size={15} /></span>
        <span className="input-card-title">Input</span>
      </span>
      <span className="input-card-what">Links, notes and files the team starts from</span>
      <CardBody items={items} room={room} />
      <span className="input-card-foot">
        {foot && <span className="input-card-to">{foot}</span>}
        <span className="input-card-open">
          {action}
          <Icon name="arrow-right" size={12} />
        </span>
      </span>
    </>
  );
}

/** Whether "Edit everything as text" is open, per browser. */
const RAW_KEY = 'sculptured.input.raw-text.v1';
function readRaw(): boolean {
  try { return globalThis.localStorage?.getItem(RAW_KEY) === 'true'; } catch { return false; }
}
function writeRaw(open: boolean) {
  try { globalThis.localStorage?.setItem(RAW_KEY, String(open)); } catch { /* A per-browser convenience only. */ }
}

export const LINK_HELP = 'Enter a web address, such as https://example.com/notes';

type Change = { text: string; onChange: (text: string) => void; locked: boolean };

/** Paste a web address, Add link (or Enter). A typo is explained, never guessed at. */
function AddLink({ text, onChange, locked }: Change) {
  const id = useId();
  const [link, setLink] = useState('');
  const [error, setError] = useState('');
  const add = () => {
    const url = normalizeLink(link);
    if (!url) return setError(LINK_HELP);
    onChange(addInputLink(text, url));
    setLink('');
    setError('');
  };
  return (
    <form className="im-form" onSubmit={(e) => { e.preventDefault(); add(); }}>
      <label className="im-label" htmlFor={`${id}-link`}><Icon name="link" size={14} />Add a link</label>
      <div className="im-row">
        <input id={`${id}-link`} className="fld-input" type="text" inputMode="url" autoComplete="off" spellCheck={false} placeholder="https://…"
          value={link} disabled={locked} aria-invalid={error ? true : undefined} aria-describedby={error ? `${id}-error` : undefined}
          onChange={(e) => { setLink(e.target.value); if (error) setError(''); }} />
        <Btn type="submit" disabled={locked || !link.trim()}>Add link</Btn>
      </div>
      {error && <p id={`${id}-error`} className="im-error tone-red" role="alert">{error}</p>}
    </form>
  );
}

/** Write a note, Add note (or Ctrl+Enter); or pick files on the agents' computer. */
function AddNote({ text, onChange, locked, onFiles }: Change & { onFiles: () => void }) {
  const id = useId();
  const [note, setNote] = useState('');
  const add = () => {
    if (!note.trim()) return;
    onChange(addInputNote(text, note));
    setNote('');
  };
  return (
    <form className="im-form" onSubmit={(e) => { e.preventDefault(); add(); }}>
      <label className="im-label" htmlFor={`${id}-note`}><Icon name="pencil" size={14} />Add a note</label>
      <TextArea id={`${id}-note`} className="fld-input im-note" rows={3} placeholder="A goal, a question, an idea…" value={note} disabled={locked}
        onChange={(e) => setNote(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); add(); } }} />
      <div className="im-actions">
        <Btn type="submit" disabled={locked || !note.trim()} kbd="Ctrl+Enter">Add note</Btn>
        <span className="im-or muted">or</span>
        <Btn icon="upload" disabled={locked} onClick={onFiles} tip="Choose files on the agents' computer: their paths are added for the agents to open">Add files…</Btn>
      </div>
    </form>
  );
}

/** One added thing: its kind as an icon, its words, and Remove. */
function ItemRow({ it, locked, onRemove }: { it: InputItem; locked: boolean; onRemove: () => void }) {
  const url = it.target ?? '';
  const label = it.kind === 'link' ? it.text.replace(url, '').replace(/^[\s:–—-]+|[\s:–—-]+$/g, '') : '';
  return (
    <li className={`im-item is-${it.kind}`}>
      <span className="im-icon" aria-hidden="true"><Icon name={KIND_ICON[it.kind]} size={14} /></span>
      <span className="im-body">
        {it.kind === 'link' && <>{label && <span className="im-text">{label}</span>}<a className="im-link" href={url} target="_blank" rel="noreferrer noopener">{shortLink(url)}</a></>}
        {it.kind === 'file' && <><span className="im-text">{fileName(url)}</span><span className="im-path mono" title={url}>{url}</span></>}
        {it.kind === 'note' && <span className="im-text im-note-text">{it.text}</span>}
      </span>
      <Btn variant="ghost" size="sm" icon="x" iconOnly disabled={locked} aria-label={`Remove ${it.kind}: ${preview(it).slice(0, 60)}`} onClick={onRemove} />
    </li>
  );
}

/** Everything added, tidy; or what to do first. */
function ItemList({ text, onChange, locked }: Change) {
  const id = useId();
  const items = useMemo(() => inputItems(text), [text]);
  const counts = countsText(inputCounts(items));
  return (
    <section className="im-list" aria-labelledby={id}>
      <h2 id={id} className="im-title">Added{counts && <span className="im-counts"> · {counts}</span>}</h2>
      {items.length === 0 ? (
        <div className="im-empty">
          <span className="im-empty-icon" aria-hidden="true"><Icon name="inbox" size={20} /></span>
          <p>Nothing added yet. Paste a link or write a first note: the team reads everything here when it starts.</p>
        </div>
      ) : (
        <ul className="im-items">
          {items.map((it) => <ItemRow key={`${it.start}-${it.kind}`} it={it} locked={locked} onRemove={() => onChange(removeInputItem(text, it))} />)}
        </ul>
      )}
    </section>
  );
}

/** The whole text as written, one step back (remembered per browser). */
function RawText({ text, onChange, locked, label }: Change & { label: string }) {
  const [open, setOpen] = useState(readRaw);
  return (
    <details className="im-raw" open={open} onToggle={(e) => { const now = (e.currentTarget as HTMLDetailsElement).open; setOpen(now); writeRaw(now); }}>
      <summary>Edit everything as text</summary>
      <p className="muted">One link or file per line; a blank line starts a new note.</p>
      <TextArea aria-label={label} className="fld-input" value={text} readOnly={locked} rows={10} maxLength={64000} onChange={(e) => onChange(e.target.value)} />
    </details>
  );
}

/**
 * Editing the Input: add a link, add a note, add files, and a tidy list of what is there, each
 * with Remove. The whole text stays one step back under "Edit everything as text", as written.
 * `lockedReason` is shown in words while locked; `rawLabel` names the raw text field; Add files… starts in `folder`.
 */
export function InputMaterial({ text, onChange, locked, lockedReason, rawLabel, folder }: Change & { lockedReason: string; rawLabel: string; folder: string | null }) {
  const [picking, setPicking] = useState(false);
  const c = { text, onChange, locked };
  return (
    <div className="input-material">
      {locked && <p className="im-locked reason" role="status">{lockedReason}</p>}
      <section className="im-add" aria-label="Add to the Input">
        <AddLink {...c} />
        <AddNote {...c} onFiles={() => setPicking(true)} />
      </section>
      <ItemList {...c} />
      <RawText {...c} label={rawLabel} />
      {picking && (
        <FolderPicker initialPath={folder} onChoose={() => undefined} onClose={() => setPicking(false)}
          pickFiles={(paths) => { onChange(withFiles(text, paths)); setPicking(false); }} />
      )}
    </div>
  );
}
