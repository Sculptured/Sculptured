import {DismissibleError} from '../shell/DismissibleError';
import { useState, type FormEvent } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { api } from '../api';
import { boardPath } from '../paths';
import { diagnosticsOf, errorText } from '../errors';
import type { Diagnostic } from '../generated/Diagnostic';
import { Btn } from '../shell/ui';
import { useRequestOnce } from '../useRequestOnce';
import { SpecNoteList, specNotes } from './SpecNotes';

type Booted = { rig: string; notes: Diagnostic[] };

/** Starts the team file at `path`; a start with warnings stays here until the operator opens the team. */
function useBoot(path: string) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [booted, setBooted] = useState<Booted | null>(null);
  // One id per start attempt: a retry after a network failure reuses it; any server answer renews it.
  const bootOnce = useRequestOnce();
  const navigate = useNavigate();
  const boot = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setBooted(null);
    try {
      const r = await bootOnce((id) => api.boot(path.trim(), id));
      const notes = specNotes(r.diagnostics ?? []);
      if (notes.some((d) => d.severity === 'warning')) setBooted({ rig: r.rig, notes });
      else navigate(boardPath(r.rig));
    } catch (err) {
      setError(err as Error);
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, booted, boot };
}

/** The expert path: team files and their problems use spec words, so all of it is under Advanced. */
export function StartFromFile({ bare = false }: { bare?: boolean }) {
  const [path, setPath] = useState('');
  const { busy, error, booted, boot } = useBoot(path);
  const bootReason = busy ? 'Starting…' : !path.trim() ? 'Enter a team file path to start.' : null;
  const body = (
    <>
      <form onSubmit={boot} className="boot-form" aria-label="Start a team from a file">
        <h2 className="sub-title">Start a team from a file</h2>
        <div className="boot-line">
          <label htmlFor="spec-path" className="prompt">
            Team file
          </label>
          <input id="spec-path" className="field" value={path} onChange={(e) => setPath(e.target.value)} placeholder="/absolute/path/to/team file" spellCheck={false} required />
          <Btn type="submit" variant="primary" icon="play" disabled={busy || !path.trim()} aria-describedby={bootReason ? 'boot-reason' : undefined}>
            Start team
          </Btn>
        </div>
        {bootReason && (
          <p id="boot-reason" className="reason">
            {bootReason}
          </p>
        )}
      </form>
      {booted && <BootWarnings booted={booted} />}
      {error && <BootError error={error} />}
    </>
  );
  if (bare) return <div className="start-from-file">{body}</div>;
  return (
    <details className="home-advanced" data-advanced>
      <summary>Advanced</summary>
      {body}
    </details>
  );
}

function BootWarnings({ booted }: { booted: Booted }) {
  return (
    <section role="status" aria-label="Start warnings" className="boot-warnings">
      <p>
        Started {booted.rig}. The team file has {booted.notes.filter((d) => d.severity === 'warning').length === 1 ? 'a warning' : 'warnings'}:
      </p>
      <SpecNoteList notes={booted.notes} />
      <Link to={boardPath(booted.rig)} className="btn btn-secondary">
        Open {booted.rig}
      </Link>
    </section>
  );
}

function BootError({ error }: { error: Error }) {
  const diagnostics = diagnosticsOf(error);
  return (
    <DismissibleError className="error tone-red">
      <p>{errorText(error)}</p>
      {diagnostics.length > 0 && (
        <ul aria-label="Team file problems">
          {diagnostics.map((d) => (
            <li key={`${d.path}-${d.message}`}>
              <code data-advanced>{d.path}</code> — {d.message}
            </li>
          ))}
        </ul>
      )}
    </DismissibleError>
  );
}
