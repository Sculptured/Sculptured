import {DismissibleError} from '../shell/DismissibleError';
import { useCallback, useEffect, useId, useRef, useState, type RefObject } from 'react';
import { api, newRequestId } from '../api';
import { errorText } from '../errors';
import type { FolderListing } from '../generated/FolderListing';
import { Btn, Icon } from '../shell/ui';
import { FileList } from './PickFiles';

export { FOLDER_WORDS } from '../errors';

/** Folder API errors in plain words, never the server's own text. */
export function folderError(e: unknown): string {
  return errorText(e);
}

let homeCache: string | null = null;
let homeLoad: Promise<string | null> | null = null;

/** The home folder, once known (one folder listing per page), for showing paths as `~/…`. */
export function useHome(): string | null {
  const [home, setHome] = useState(homeCache);
  useEffect(() => {
    if (homeCache) return;
    let live = true;
    homeLoad ??= Promise.resolve()
      .then(() => api.listFolders())
      .then((l) => (homeCache = l?.home ?? null))
      .catch(() => {
        homeLoad = null;
        return null;
      });
    void homeLoad.then((h) => live && h && setHome(h));
    return () => {
      live = false;
    };
  }, []);
  return home;
}

/** `/home/me/projects/x` → `~/projects/x` when it is inside home. */
export function shortPath(path: string, home: string | null): string {
  if (!home) return path;
  if (path === home) return '~';
  return path.startsWith(`${home}/`) ? `~/${path.slice(home.length + 1)}` : path;
}

type Props = {
  /** Hosted: browse the computer that runs this team (its allowed folders). */
  team?: string;
  /** Hosted: browse this computer (an agent's own) instead of the team's. */
  host?: string;
  initialPath: string | null;
  onChoose: (path: string) => void;
  /** Choosing files instead of a folder: ticks persist across folders; gets their full paths. */
  pickFiles?: (paths: string[]) => void;
  onClose: () => void;
};

type Place = { host: string } | { team: string } | undefined;

/** Opens a listing at a path (or home), with a plain-words error when it cannot be. */
function useOpenListing(on: Place, team: string | undefined, pickFiles: Props['pickFiles']) {
  const [listing, setListing] = useState<FolderListing | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const open = useCallback(async (path?: string) => {
    setBusy(true);
    setError(null);
    try {
      const l = await (pickFiles ? api.listFolders(path, on ?? {}, true) : on ? api.listFolders(path, on) : api.listFolders(path));
      if (!on) homeCache = l.home;
      setListing(l);
      return true;
    } catch (e) {
      setError(folderError(e));
      return false;
    } finally {
      setBusy(false);
    }
  }, [team]); // eslint-disable-line react-hooks/exhaustive-deps
  return { listing, setListing, busy, error, setError, open };
}

/** The listing being browsed: the saved folder when it can be shown, else home with a note saying so. */
function useFolderBrowser(on: Place, team: string | undefined, initialPath: string | null, pickFiles: Props['pickFiles'], dialog: RefObject<HTMLDivElement | null>) {
  const { listing, setListing, busy, error, setError, open } = useOpenListing(on, team, pickFiles);
  const [note, setNote] = useState<string | null>(null);
  const opener = useRef<Element | null>(null);

  /** The saved folder can't be shown (moved, deleted, outside home): start at home, and say so. */
  const startAtHome = async (initial: string) => {
    try {
      const l = await (on ? api.listFolders(undefined, on) : api.listFolders());
      setListing(l);
      setNote(`${shortPath(initial, l.home)} could not be shown, so this starts in your home folder.`);
    } catch (e) {
      setError(folderError(e));
    }
  };

  useEffect(() => {
    opener.current = document.activeElement;
    void (async () => {
      if (!initialPath) return void (await open());
      if (!(await open(initialPath))) await startAtHome(initialPath);
    })();
    return () => {
      if (opener.current instanceof HTMLElement && document.contains(opener.current)) opener.current.focus();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!busy) dialog.current?.querySelector<HTMLElement>('.folder-row, [data-autofocus]')?.focus();
    // Only on first load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [listing === null]);

  const go = async (path: string) => {
    setNote(null);
    const ok = await open(path || undefined);
    if (ok) dialog.current?.querySelector<HTMLElement>('.folder-list')?.scrollTo?.({ top: 0 });
  };
  return { listing, busy, error, note, go };
}

/** Making a new folder in the listing; a fresh request id per attempt, because a refused name stores its refusal under its own id. */
function useNewFolder(listing: FolderListing | null, on: Place, go: (path: string) => Promise<void>) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [createError, setCreateError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);
  const create = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!listing || !name.trim()) return;
    setCreateBusy(true);
    setCreateError(null);
    try {
      const entry = await (on ? api.createFolder(listing.path, name.trim(), newRequestId(), on) : api.createFolder(listing.path, name.trim(), newRequestId()));
      setCreating(false);
      setName('');
      await go(entry.path);
    } catch (err) {
      setCreateError(folderError(err));
    } finally {
      setCreateBusy(false);
    }
  };
  return { creating, setCreating, name, setName, createError, createBusy, create };
}

/** Tab stays inside the dialog. */
function trapTab(e: React.KeyboardEvent, root: HTMLElement | null) {
  const f = [...(root?.querySelectorAll<HTMLElement>('button:not(:disabled), input, [tabindex="0"]') ?? [])];
  if (f.length === 0) return;
  const i = f.indexOf(document.activeElement as HTMLElement);
  if (e.shiftKey && i <= 0) {
    e.preventDefault();
    f[f.length - 1].focus();
  } else if (!e.shiftKey && i === f.length - 1) {
    e.preventDefault();
    f[0].focus();
  }
}

type NewFolder = ReturnType<typeof useNewFolder>;

/** The computer to browse: an agent's own, the team's, or this one. */
const placeOf = (host: string | undefined, team: string | undefined): Place => (host ? { host } : team ? { team } : undefined);

const toggle = (list: string[], path: string, on: boolean) => (on ? [...list, path] : list.filter((x) => x !== path));

function PickerNotes({ note, error }: { note: string | null; error: string | null }) {
  return <>
    {note && <p className="fp-note tone-amber" role="status">{note}</p>}
    {error && <DismissibleError className="fp-note tone-red">{error}</DismissibleError>}
  </>;
}

/** Esc closes the form for a new folder, else the picker; Tab stays inside. */
function pickerKeys(dialog: RefObject<HTMLDivElement | null>, nf: NewFolder, onClose: () => void) {
  return (e: React.KeyboardEvent) => {
    if (e.key === 'Tab') return trapTab(e, dialog.current);
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    if (nf.creating) nf.setCreating(false);
    else onClose();
  };
}

function NewFolderForm({ ids, here, nf, input }: { ids: string; here: string; nf: NewFolder; input: RefObject<HTMLInputElement | null> }) {
  return (
    <form className="fp-create" onSubmit={nf.create} aria-label="New folder">
      <label htmlFor={`${ids}-name`} className="prompt">
        new folder in {here}
      </label>
      <input id={`${ids}-name`} ref={input} className="field" value={nf.name} spellCheck={false} onChange={(e) => nf.setName(e.target.value)} aria-describedby={nf.createError ? `${ids}-cerr` : undefined} />
      <Btn type="submit" disabled={nf.createBusy || !nf.name.trim()}>
        Create
      </Btn>
      <Btn onClick={() => nf.setCreating(false)}>Cancel</Btn>
      {nf.createError && (
        <DismissibleError id={`${ids}-cerr`} className="tone-red fp-cerr">
          {nf.createError}
        </DismissibleError>
      )}
    </form>
  );
}

function FolderRows({ listing, here, busy, go }: { listing: FolderListing | null; here: string; busy: boolean; go: (path: string) => void }) {
  return (
    <ul className="folder-list" aria-label={`Folders in ${here}`} aria-busy={busy}>
      {listing && listing.entries.length === 0 && !busy && <li className="muted fp-empty">No folders here.</li>}
      {listing?.entries.map((f) => (
        <li key={f.path}>
          <button type="button" className="folder-row" onClick={() => go(f.path)} aria-label={`Open ${f.name}`}>
            <span className="fp-glyph" aria-hidden="true">
              <Icon name="folder" size={15} />
            </span>
            <span className="fp-name">{f.name}/</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

type FootProps = { nf: NewFolder; pickFiles: Props['pickFiles']; chosen: string[]; listing: FolderListing | null; busy: boolean; top: boolean; here: string; onChoose: (path: string) => void; onClose: () => void };

function PickerFoot({ nf, pickFiles, chosen, listing, busy, top, here, onChoose, onClose }: FootProps) {
  return (
    <div className="fp-foot">
      {!nf.creating && !pickFiles && (
        <Btn icon="folder-plus" onClick={() => nf.setCreating(true)} disabled={!listing || top}>
          New folder
        </Btn>
      )}
      <span className="pane-spacer" />
      <Btn onClick={onClose}>Cancel</Btn>
      {pickFiles ? (
        <Btn variant="primary" icon="check" onClick={() => pickFiles(chosen)} disabled={chosen.length === 0} data-autofocus>
          Add {chosen.length} {chosen.length === 1 ? 'file' : 'files'}
        </Btn>
      ) : (
        <Btn variant="primary" icon="check" onClick={() => listing && onChoose(listing.path)} disabled={!listing || busy || top} data-autofocus>
          Use {here}
        </Btn>
      )}
    </div>
  );
}

/** Where the picker is: its label, whether it is the top (the allowed folders), and what Up does. On a
 * connected computer each allowed folder acts as home, so paths show in full and Up from one goes back to
 * its top level, which is not a folder itself. */
function pathBar(listing: FolderListing | null, on: Place, go: (path: string) => Promise<void>) {
  const top = listing !== null && listing.path === '';
  const home = on ? null : (listing?.home ?? null);
  const here = !listing ? '…' : top ? 'Folders this computer allows' : shortPath(listing.path, home);
  const upTo = upAction(listing, top, on, go);
  const upReason = listing && !top && !upTo ? 'This is your home folder.' : null;
  return { here, top, upTo, upReason };
}

/** What Up does: to the parent folder, to the top level of a connected computer, or nothing. */
function upAction(listing: FolderListing | null, top: boolean, on: Place, go: (path: string) => Promise<void>): (() => void) | null {
  if (!listing || top) return null;
  if (listing.parent) return () => void go(listing.parent!);
  return on ? () => void go('') : null;
}

function PathBar({ ids, bar, listing, busy }: { ids: string; bar: ReturnType<typeof pathBar>; listing: FolderListing | null; busy: boolean }) {
  return (
    <div className="fp-path">
      <Btn icon="arrow-up" onClick={() => bar.upTo?.()} disabled={!bar.upTo || busy} aria-describedby={bar.upReason ? `${ids}-up` : undefined}>
        Up
      </Btn>
      <span className="fp-here" title={listing?.path}>
        {bar.here}
      </span>
      {bar.upReason && <span id={`${ids}-up`} className="reason">{bar.upReason}</span>}
    </div>
  );
}

/**
 * Chooses a folder inside the home folder. Lists folders only (never files), can make a new
 * folder, and says what went wrong in plain words. Esc closes and focus returns to the opener.
 */
export function FolderPicker({ team, host, initialPath, onChoose, pickFiles, onClose }: Props) {
  const on = placeOf(host, team);
  const ids = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const { listing, busy, error, note, go } = useFolderBrowser(on, team, initialPath, pickFiles, dialog);
  const nf = useNewFolder(listing, on, go);
  useEffect(() => { if (nf.creating) nameInput.current?.focus(); }, [nf.creating]);
  const onKeyDown = pickerKeys(dialog, nf, onClose);
  const bar = pathBar(listing, on, go);
  const { here } = bar;
  return (
    <div className="overlay" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={dialog} role="dialog" aria-modal="true" aria-labelledby={`${ids}-title`} className="panel folder-picker" onKeyDown={onKeyDown}>
        <div className="panel-bar">
          <span className="panel-title" id={`${ids}-title`}>
            {pickFiles ? 'Choose files' : 'Choose a folder'}
          </span>
          <span className="panel-rule" aria-hidden="true" />
          <Btn onClick={onClose} variant="ghost" icon="x" iconOnly aria-label="Close" className="panel-x" kbd="esc" />
        </div>
        <PathBar ids={ids} bar={bar} listing={listing} busy={busy} />
        <PickerNotes note={note} error={error} />
        <FolderRows listing={listing} here={here} busy={busy} go={(p) => void go(p)} />
        {listing?.truncated && <p className="fp-note muted">Only the first folders are shown.</p>}
        {pickFiles && listing && !bar.top && !busy && <FileList files={listing.files} chosen={chosen} onToggle={(path, tick) => setChosen((c) => toggle(c, path, tick))} />}
        {nf.creating ? <NewFolderForm ids={ids} here={here} nf={nf} input={nameInput} /> : null}
        <PickerFoot nf={nf} pickFiles={pickFiles} chosen={chosen} listing={listing} busy={busy} top={bar.top} here={here} onChoose={onChoose} onClose={onClose} />
      </div>
    </div>
  );
}
