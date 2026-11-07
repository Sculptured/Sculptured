import {DismissibleError} from '../shell/DismissibleError';
import { errorText } from '../errors';
import { useId, useRef, useState } from 'react';
import { api } from '../api';
import { downloadText } from '../download';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import type { TeamSpec } from '../generated/TeamSpec';
import { MARK } from '../draft/issues';
import { Btn } from '../shell/ui';
import { WORDS } from '../vocab';
import { plural } from '../format';
import '../features.css';

type Props = {
  teamId: string;
  draft: TeamDraft;
  /** Saves pending edits first, so the spec is the team on screen. */
  flush: () => Promise<{ ok: boolean }>;
};

const list = (names: string[]) => (names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`);

/** "This team needs a folder for Builder and Reviewer before it can run." from the spec check. */
export function needsSentence(draft: TeamDraft, issues: DraftIssue[]): string {
  const folders = issues
    .filter((i) => i.severity === 'error' && i.code === 'folder-missing' && i.element.kind === 'agent')
    .map((i) => draft.agents.find((a) => a.id === i.element.id)?.name.trim() || 'an unnamed agent');
  const others = issues.filter((i) => i.severity === 'error' && i.code !== 'folder-missing').length;
  if (folders.length > 0)
    return `This team needs a folder for ${list([...new Set(folders)])}${others ? `, and ${plural(others, 'more fix', 'more fixes')},` : ''} before it can run.`;
  return 'This team has things to fix before it can run.';
}

/** What an export reports: busy while it saves and reads, an error, or the file it wrote. */
function useExportStatus() {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [exported, setExported] = useState<string | null>(null);
  /** A new export starts: busy, with nothing reported yet. */
  const begin = () => { setBusy(true); setError(null); setExported(null); };
  return { error, setError, busy, setBusy, exported, setExported, begin };
}

type Status = ReturnType<typeof useExportStatus>;

/** Select the text of an element, so it can be copied by hand. */
function selectAll(el: HTMLElement | null) {
  const sel = window.getSelection();
  if (!el || !sel) return;
  sel.selectAllChildren(el);
  el.focus();
}

/** The team's spec: read after saving, shown read-only, copied, or downloaded as a file. */
function useSpecActions(teamId: string, flush: Props['flush'], st: Status) {
  const [spec, setSpec] = useState<TeamSpec | null>(null);
  const [copied, setCopied] = useState(false);
  const [unsaved, setUnsaved] = useState(false);
  const pre = useRef<HTMLPreElement>(null);
  const file = `${teamId}.rig.yaml`;
  const load = async (): Promise<TeamSpec | null> => {
    st.begin();
    setCopied(false);
    try {
      const saved = await flush();
      setUnsaved(!saved.ok);
      const s = await api.teamSpec(teamId);
      setSpec(s);
      return s;
    } catch (e) {
      st.setError(`Could not read the spec: ${errorText(e)}`);
      return null;
    } finally {
      st.setBusy(false);
    }
  };
  const exportSpec = async () => {
    const s = await load();
    if (!s) return;
    downloadText(file, s.yaml);
    st.setExported(file);
  };
  const copy = async () => {
    if (!spec) return;
    try {
      await navigator.clipboard.writeText(spec.yaml);
      setCopied(true);
    } catch {
      st.setError('Copy did not work. The text is selected below: copy it with Ctrl+C.');
      selectAll(pre.current);
    }
  };
  return { spec, copied, unsaved, pre, file, load, exportSpec, copy };
}

/** The team file (MADL), saved first so the file is the saved design. */
function useMadlExport(teamId: string, flush: Props['flush'], st: Status) {
  const [withFolders, setWithFolders] = useState(false);
  const exportMadl = async () => {
    st.begin();
    try {
      await flush();
      const { yaml } = await api.teamMadl(teamId, withFolders);
      const name = `${teamId}.madl.yaml`;
      // The first line lets the file run from a terminal (`./team.madl.yaml`, after chmod +x); to MADL
      // tools it is a YAML comment.
      downloadText(name, yaml.startsWith('#!') ? yaml : `#!/usr/bin/env -S sculptured run\n${yaml}`, 'application/yaml');
      st.setExported(name);
    } catch (e) {
      st.setError(`Could not export the team: ${errorText(e)}`);
    } finally {
      st.setBusy(false);
    }
  };
  return { withFolders, setWithFolders, exportMadl };
}

function useTeamExport(teamId: string, flush: Props['flush']) {
  const st = useExportStatus();
  return { ...st, ...useSpecActions(teamId, flush, st), ...useMadlExport(teamId, flush, st) };
}

/**
 * Export the team (a team file, or MADL), and under Advanced its spec, read-only with Copy.
 * Spec words are allowed there only (`data-advanced`).
 */
export function TeamExport({ teamId, draft, flush }: Props) {
  const ids = useId();
  const x = useTeamExport(teamId, flush);
  return (
    <div className="team-export">
      <div className="ta-actions">
        <Btn onClick={() => void x.exportMadl()} disabled={x.busy} title="A MADL (Multi-Agent Descriptor Language) file, which other tools can read too">Export team file</Btn>
        <label className="ta-check"><input type="checkbox" checked={x.withFolders} onChange={(e) => x.setWithFolders(e.target.checked)} /> Include folders (paths on this computer)</label>
      </div>
      <p className="ta-status" role="status">
        {x.busy ? <span id={`${ids}-busy`}>Saving and reading the spec…</span> : x.exported ? <>Exported {x.exported}. It is in your downloads.</> : null}
      </p>
      {x.error && <DismissibleError className="tone-red ta-error">{x.error}</DismissibleError>}
      <details className="team-advanced" data-advanced>
        <summary>{WORDS.advanced}</summary>
        <div className="ta-body">
          <p className="ta-hint">See this team as a spec file, or save it.</p>
          <div className="ta-actions">
            <Btn onClick={() => void x.load()} disabled={x.busy} aria-describedby={x.busy ? `${ids}-busy` : undefined}>View as spec</Btn>
            <Btn onClick={() => void x.exportSpec()} disabled={x.busy} aria-describedby={x.busy ? `${ids}-busy` : undefined}>Export</Btn>
          </div>
          {x.spec && <SpecView spec={x.spec} draft={draft} file={x.file} unsaved={x.unsaved} copied={x.copied} onCopy={() => void x.copy()} pre={x.pre} />}
        </div>
      </details>
    </div>
  );
}

type SpecProps = { spec: TeamSpec; draft: TeamDraft; file: string; unsaved: boolean; copied: boolean; onCopy: () => void; pre: React.RefObject<HTMLPreElement | null> };

function SpecView({ spec, draft, file, unsaved, copied, onCopy, pre }: SpecProps) {
  return (
    <section aria-label="Team spec" className="ta-spec">
      <p className="ta-hint">This is the OpenRig format Sculptured uses internally. It is read-only here; names, jobs and positions are kept in the last comment line.</p>
      {unsaved && <p className="tone-amber">Your latest edits are not saved yet, so this is the last saved version.</p>}
      {!spec.valid && <p className="tone-amber ta-needs">{needsSentence(draft, spec.issues)}</p>}
      {spec.issues.length > 0 && (
        <ul className="ta-issues" aria-label="Spec check">
          {spec.issues.map((i, n) => (
            <li key={`${i.code}-${i.element.id}-${n}`}>
              <span className={i.severity === 'error' ? 'tone-red' : 'tone-amber'} aria-hidden="true">{MARK[i.severity]} </span>
              {i.message}
              {i.spec_path && <> <code className="ta-path">{i.spec_path}</code></>}
              {i.detail && <span className="muted"> · {i.detail}</span>}
            </li>
          ))}
        </ul>
      )}
      <div className="ta-file">
        <span className="ta-name">{file}</span>
        <span className="ta-file-tools">{copied && <span role="status" className="tone-green">Copied</span>}<Btn onClick={onCopy}>Copy</Btn></span>
      </div>
      <pre ref={pre} className="ta-yaml" tabIndex={0} aria-label={`${file}, read-only`}>{spec.yaml}</pre>
    </section>
  );
}
