import {DismissibleError} from '../shell/DismissibleError';
import { useCallback, useEffect, useId, useRef, useState } from 'react';
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

/**
 * Chooses a folder inside the home folder. Lists folders only (never files), can make a new
 * folder, and says what went wrong in plain words. Esc closes and focus returns to the opener.
 */
export function FolderPicker({ team, host, initialPath, onChoose, pickFiles, onClose }: Props) {
  const on = host ? { host } : team ? { team } : undefined;
  const ids = useId();
  const [listing, setListing] = useState<FolderListing | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [createError, setCreateError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const [chosen, setChosen] = useState<string[]>([]);

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

  useEffect(() => {
    opener.current = document.activeElement;
    void (async () => {
      if (initialPath) {
        const ok = await open(initialPath);
        if (!ok) {
          // The saved folder can't be shown (moved, deleted, outside home): start at home, and say so.
          try {
            const l = await (on ? api.listFolders(undefined, on) : api.listFolders());
            setListing(l);
            setNote(`${shortPath(initialPath, l.home)} could not be shown, so this starts in your home folder.`);
          } catch (e) {
            setError(folderError(e));
          }
        }
      } else await open();
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

  useEffect(() => {
    if (creating) nameInput.current?.focus();
  }, [creating]);

  const go = async (path: string) => {
    setNote(null);
    const ok = await open(path || undefined);
    if (ok) dialog.current?.querySelector<HTMLElement>('.folder-list')?.scrollTo?.({ top: 0 });
  };

  const create = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!listing || !name.trim()) return;
    setCreateBusy(true);
    setCreateError(null);
    try {
      // A fresh id per attempt: a refused name stores its refusal under its own id.
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

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (creating) setCreating(false);
      else onClose();
      return;
    }
    if (e.key !== 'Tab') return;
    // Keep focus inside the dialog.
    const f = [...(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input, [tabindex="0"]') ?? [])];
    if (f.length === 0) return;
    const i = f.indexOf(document.activeElement as HTMLElement);
    if (e.shiftKey && i <= 0) {
      e.preventDefault();
      f[f.length - 1].focus();
    } else if (!e.shiftKey && i === f.length - 1) {
      e.preventDefault();
      f[0].focus();
    }
  };

  // On a connected computer, each allowed folder acts as home, so paths show in full and Up from one
  // goes back to its top level: the allowed folders, which is not a folder itself.
  const home = on ? null : (listing?.home ?? null);
  const top = listing !== null && listing.path === '';
  const here = listing ? (top ? 'Folders this computer allows' : shortPath(listing.path, home)) : '…';
  const upTo: (() => void) | null = !listing || top ? null : listing.parent ? () => void go(listing.parent!) : on ? () => void go('') : null;
  const upReason = listing && !top && !upTo ? 'This is your home folder.' : null;

  return (
    <div className="overlay" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${ids}-title`}
        className="panel folder-picker"
        onKeyDown={onKeyDown}
      >
        <div className="panel-bar">
          <span className="panel-title" id={`${ids}-title`}>
            {pickFiles ? 'Choose files' : 'Choose a folder'}
          </span>
          <span className="panel-rule" aria-hidden="true" />
          <Btn onClick={onClose} variant="ghost" icon="x" iconOnly aria-label="Close" className="panel-x" kbd="esc" />
        </div>
        <div className="fp-path">
          <Btn icon="arrow-up" onClick={() => upTo?.()} disabled={!upTo || busy} aria-describedby={upReason ? `${ids}-up` : undefined}>
            Up
          </Btn>
          <span className="fp-here" title={listing?.path}>
            {here}
          </span>
          {upReason && (
            <span id={`${ids}-up`} className="reason">
              {upReason}
            </span>
          )}
        </div>
        {note && (
          <p className="fp-note tone-amber" role="status">
            {note}
          </p>
        )}
        {error && (
          <DismissibleError className="fp-note tone-red">
            {error}
          </DismissibleError>
        )}
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
        {listing?.truncated && <p className="fp-note muted">Only the first folders are shown.</p>}
        {pickFiles && listing && !top && !busy && (
          <FileList files={listing.files} chosen={chosen} onToggle={(path, tick) => setChosen((c) => (tick ? [...c, path] : c.filter((x) => x !== path)))} />
        )}
        {creating ? (
          <form className="fp-create" onSubmit={create} aria-label="New folder">
            <label htmlFor={`${ids}-name`} className="prompt">
              new folder in {here}
            </label>
            <input
              id={`${ids}-name`}
              ref={nameInput}
              className="field"
              value={name}
              spellCheck={false}
              onChange={(e) => setName(e.target.value)}
              aria-describedby={createError ? `${ids}-cerr` : undefined}
            />
            <Btn type="submit" disabled={createBusy || !name.trim()}>
              Create
            </Btn>
            <Btn onClick={() => setCreating(false)}>Cancel</Btn>
            {createError && (
              <DismissibleError id={`${ids}-cerr`} className="tone-red fp-cerr">
                {createError}
              </DismissibleError>
            )}
          </form>
        ) : null}
        <div className="fp-foot">
          {!creating && !pickFiles && (
            <Btn icon="folder-plus" onClick={() => setCreating(true)} disabled={!listing || top}>
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
      </div>
    </div>
  );
}
