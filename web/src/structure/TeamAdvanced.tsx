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
import { TeamLimitsFields } from './TeamLimitsFields';
import { BudgetFields } from './BudgetFields';
import { ClaudeSetupFields } from './ClaudeSetupFields';
import { ProtectionFields } from './ProtectionFields';
import { ComponentsSection } from './ComponentsSection';
import { ImproveFromRun } from './ImproveFromRun';
import type { Edit } from './Panels';
import '../features.css';

type Props = {
  teamId: string;
  draft: TeamDraft;
  /** Saves pending edits first, so the spec is the team on screen. */
  flush: () => Promise<{ ok: boolean }>;
  /** Changes the team's own settings (its limits); absent = read-only. */
  edit?: Edit;
  locked?: boolean;
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

/**
 * Expert tools for a team, closed by default: its spec, read-only with Copy, and Export.
 * Spec words are allowed here only (`data-advanced`).
 */
export function TeamAdvanced({ teamId, draft, flush, edit, locked = false }: Props) {
  const ids = useId();
  const [spec, setSpec] = useState<TeamSpec | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [unsaved, setUnsaved] = useState(false);
  const [exported, setExported] = useState<string | null>(null);
  const [withFolders, setWithFolders] = useState(false);
  const pre = useRef<HTMLPreElement>(null);
  const file = `${teamId}.rig.yaml`;

  const load = async (): Promise<TeamSpec | null> => {
    setBusy(true);
    setError(null);
    setCopied(false);
    setExported(null);
    try {
      const saved = await flush();
      setUnsaved(!saved.ok);
      const s = await api.teamSpec(teamId);
      setSpec(s);
      return s;
    } catch (e) {
      setError(`Could not read the spec: ${errorText(e)}`);
      return null;
    } finally {
      setBusy(false);
    }
  };

  const exportSpec = async () => {
    const s = await load();
    if (!s) return;
    downloadText(file, s.yaml);
    setExported(file);
  };

  const exportFile = async () => {
    setBusy(true);
    setError(null);
    setExported(null);
    try {
      await flush();
      const f = await api.teamFile(teamId, withFolders);
      const name = `${teamId}.sculptured-team.json`;
      downloadText(name, JSON.stringify(f, null, 2), 'application/json');
      setExported(name);
    } catch (e) {
      setError(`Could not export the team: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const exportMadl = async () => {
    setBusy(true);
    setError(null);
    setExported(null);
    try {
      await flush();
      const { yaml } = await api.teamMadl(teamId, withFolders);
      const name = `${teamId}.madl.yaml`;
      downloadText(name, yaml, 'application/yaml');
      setExported(name);
    } catch (e) {
      setError(`Could not export the team: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const copy = async () => {
    if (!spec) return;
    try {
      await navigator.clipboard.writeText(spec.yaml);
      setCopied(true);
    } catch (e) {
      setError('Copy did not work. The text is selected below: copy it with Ctrl+C.');
      const sel = window.getSelection();
      if (pre.current && sel) {
        sel.selectAllChildren(pre.current);
        pre.current.focus();
      }
    }
  };

  return (
    <details className="team-advanced" data-advanced>
      <summary>{WORDS.advanced}</summary>
      <div className="ta-body">
        {edit && <BudgetFields draft={draft} edit={edit} locked={locked} />}
        {edit && <ClaudeSetupFields draft={draft} edit={edit} locked={locked} />}
        {edit && <ProtectionFields draft={draft} edit={edit} locked={locked} />}
        {edit && <ComponentsSection draft={draft} edit={edit} locked={locked} />}
        {edit && <TeamLimitsFields draft={draft} edit={edit} locked={locked} />}
        {edit && <ImproveFromRun teamId={teamId} draft={draft} edit={edit} locked={locked} />}
        <p className="ta-hint">See this team as a spec file, or save it to share or keep.</p>
        <div className="ta-actions">
          <Btn onClick={() => void load()} disabled={busy} aria-describedby={busy ? `${ids}-busy` : undefined}>
            View as spec
          </Btn>
          <Btn onClick={() => void exportSpec()} disabled={busy} aria-describedby={busy ? `${ids}-busy` : undefined}>
            Export
          </Btn>
        </div>
        <div className="ta-actions">
          <Btn onClick={() => void exportFile()} disabled={busy}>
            Export team file
          </Btn>
          <Btn onClick={() => void exportMadl()} disabled={busy} title="A Multi-Agent Descriptor Language file other tools can read">
            Export as MADL
          </Btn>
          <label className="ta-check">
            <input type="checkbox" checked={withFolders} onChange={(e) => setWithFolders(e.target.checked)} /> Include folders (paths on this computer)
          </label>
        </div>
        <p className="ta-status" role="status">
          {busy ? (
            <span id={`${ids}-busy`}>Saving and reading the spec…</span>
          ) : exported ? (
            <>Exported {exported}. It is in your downloads.</>
          ) : null}
        </p>
        {error && (
          <DismissibleError className="tone-red ta-error">
            {error}
          </DismissibleError>
        )}
        {spec && (
          <section aria-label="Team spec" className="ta-spec">
            <p className="ta-hint">
              This is the OpenRig format Sculptured uses internally. It is read-only here; names, jobs and positions are kept in the last comment
              line.
            </p>
            {unsaved && <p className="tone-amber">Your latest edits are not saved yet, so this is the last saved version.</p>}
            {!spec.valid && <p className="tone-amber ta-needs">{needsSentence(draft, spec.issues)}</p>}
            {spec.issues.length > 0 && (
              <ul className="ta-issues" aria-label="Spec check">
                {spec.issues.map((i, n) => (
                  <li key={`${i.code}-${i.element.id}-${n}`}>
                    <span className={i.severity === 'error' ? 'tone-red' : 'tone-amber'} aria-hidden="true">
                      {MARK[i.severity]}{' '}
                    </span>
                    {i.message}
                    {i.spec_path && (
                      <>
                        {' '}
                        <code className="ta-path">{i.spec_path}</code>
                      </>
                    )}
                    {i.detail && <span className="muted"> · {i.detail}</span>}
                  </li>
                ))}
              </ul>
            )}
            <div className="ta-file">
              <span className="ta-name">{file}</span>
              <span className="ta-file-tools">
                {copied && (
                  <span role="status" className="tone-green">
                    Copied
                  </span>
                )}
                <Btn onClick={() => void copy()}>Copy</Btn>
              </span>
            </div>
            <pre ref={pre} className="ta-yaml" tabIndex={0} aria-label={`${file}, read-only`}>
              {spec.yaml}
            </pre>
          </section>
        )}
      </div>
    </details>
  );
}
