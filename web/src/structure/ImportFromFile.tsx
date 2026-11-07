import { DismissibleError } from '../shell/DismissibleError';
import { TextArea } from '../shell/TextArea';
import { diagnosticsOf, errorText } from '../errors';
import { ApiError, api } from '../api';
import type { DraftIssue } from '../generated/DraftIssue';
import { useId, useState } from 'react';
import { useRequestOnce } from '../useRequestOnce';
import type { Diagnostic } from '../generated/Diagnostic';
import type { TeamImport } from '../generated/TeamImport';
import { Btn } from '../shell/ui';
import { isMadl } from './madl';
import { plural } from '../format';

type ImportError = { message: string; diagnostics: Diagnostic[]; issues?: DraftIssue[] };

/** A MADL file (YAML or JSON), a Sculptured team file (JSON), or else an OpenRig YAML spec.
 * JSON is a MADL document or an older Sculptured team file; the server reads both. A JSON file that does not parse is refused here. */
function jsonProblem(yaml: string): string | null {
  const braces = yaml.trim().startsWith('{');
  if (!braces) return null;
  try {
    JSON.parse(yaml);
    return null;
  } catch (e) {
    return `This is not valid JSON: ${(e as Error).message}`;
  }
}

function useImportFile(onCreated: (id: string) => void) {
  const [yaml, setYaml] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ImportError | null>(null);
  const [done, setDone] = useState<TeamImport | null>(null);
  const importOnce = useRequestOnce();
  const readFile = async (f: File | undefined) => {
    if (f) setYaml(await f.text());
  };
  const run = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      const problem = jsonProblem(yaml);
      if (problem) return setError({ message: problem, diagnostics: [] });
      const madl = isMadl(yaml) || yaml.trim().startsWith('{');
      const r = await importOnce((id) => (madl ? api.importMadl(yaml, id) : api.importTeam(yaml, id)));
      if (r.dropped.length === 0) onCreated(r.team.draft.id);
      else setDone(r);
    } catch (err) {
      setError({ message: errorText(err), diagnostics: diagnosticsOf(err), issues: err instanceof ApiError ? err.issues : undefined });
    } finally {
      setBusy(false);
    }
  };
  return { yaml, setYaml, busy, error, done, readFile, run };
}

function ImportErrors({ error }: { error: ImportError }) {
  return (
    <DismissibleError className="tone-red">
      <p>{error.message}</p>
      {error.diagnostics.length > 0 && (
        <ul aria-label="Problems in the file">
          {error.diagnostics.map((d, i) => (
            <li key={i}>
              <code>{d.path}</code> — {d.message}
            </li>
          ))}
        </ul>
      )}
      {error.issues && error.issues.length > 0 && (
        <ul aria-label="Issues in the team">
          {error.issues.map((i, n) => (
            <li key={n}>{i.message}</li>
          ))}
        </ul>
      )}
    </DismissibleError>
  );
}

function ImportDropped({ done, onCreated }: { done: TeamImport; onCreated: (id: string) => void }) {
  return (
    <div role="status" className="import-dropped">
      <p>
        Imported {done.team.draft.name}. {plural(done.dropped.length, 'part was', 'parts were')} not imported:
      </p>
      <ul aria-label="Not imported">
        {done.dropped.map((d, i) => (
          <li key={i}>
            <code>{d.path}</code> — {d.message}
          </li>
        ))}
      </ul>
      <Btn onClick={() => onCreated(done.team.draft.id)}>Open {done.team.draft.name}</Btn>
    </div>
  );
}

export function ImportFromFile({ onCreated }: { onCreated: (id: string) => void }) {
  const ids = useId();
  const f = useImportFile(onCreated);
  return (
    <form className="import-form" onSubmit={f.run} aria-label="Import a team from a file">
      <p className="muted">Import a team file (.madl.yaml or .madl.json, or an older .sculptured-team.json) or an OpenRig YAML file. Anything that can't be kept is listed.</p>
      <div className="import-line">
        <label htmlFor={`${ids}-file`} className="prompt">
          file
        </label>
        <input id={`${ids}-file`} type="file" accept=".json,.yaml,.yml,application/json,text/yaml" onChange={(e) => void f.readFile(e.target.files?.[0])} />
      </div>
      <label htmlFor={`${ids}-yaml`} className="sr-only">
        Team file contents
      </label>
      <TextArea id={`${ids}-yaml`} className="fld-input import-yaml" rows={6} value={f.yaml} spellCheck={false} placeholder="…or paste the file here" onChange={(e) => f.setYaml(e.target.value)} />
      <Btn type="submit" disabled={f.busy || !f.yaml.trim()}>
        Import
      </Btn>
      {f.error && <ImportErrors error={f.error} />}
      {f.done && <ImportDropped done={f.done} onCreated={onCreated} />}
    </form>
  );
}
