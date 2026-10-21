import { useEffect, useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { TeamSettings } from '../generated/TeamSettings';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Edit } from './Panels';
import { inheritProjectFolder, setProjectFolder } from '../draft/folders';
import { FolderPicker, shortPath, useHome } from './FolderPicker';
import { Btn } from '../shell/ui';
import { useHosted } from '../hosted/HostedGate';
import { RunOn } from '../hosted/RunOn';

export function ProjectFolder({ draft, edit, locked }: { draft: TeamDraft; edit: Edit; locked: boolean }) {
  const hosted = useHosted().kind === 'signed-in';
  if (hosted) return <HostedFolder draft={draft} edit={edit} locked={locked} />;
  return <LocalFolder draft={draft} edit={edit} locked={locked} />;
}

/** Hosted: agents run on your computer, so the folder is a path there (checked by that computer). */
function HostedFolder({ draft, edit, locked }: { draft: TeamDraft; edit: Edit; locked: boolean }) {
  const [value, setValue] = useState(draft.folder ?? '');
  const [picking, setPicking] = useState(false);
  useEffect(() => setValue(draft.folder ?? ''), [draft.folder]);
  const save = () => { const v = value.trim(); if (v !== (draft.folder ?? '')) edit(d => setProjectFolder(d, v || null)); };
  return <section className="fld" aria-label="Project folder">
    <label className="fld-label" htmlFor={`folder-${draft.id}`}>Folder on your computer</label>
    <input id={`folder-${draft.id}`} className="fld-input mono" placeholder="/home/you/project" value={value} disabled={locked}
      onChange={e => setValue(e.target.value)} onBlur={save} onKeyDown={e => { if (e.key === 'Enter') save(); }} />
    <Btn disabled={locked} onClick={() => setPicking(true)}>Browse…</Btn>
    {picking && <FolderPicker team={draft.id} initialPath={draft.folder ?? null} onClose={() => setPicking(false)} onChoose={folder => {
      edit(d => setProjectFolder(d, folder)); setPicking(false);
    }} />}
    <p className="fld-hint muted">A full path on the computer that runs this team. It must be inside a folder that computer allows (<code>sculptured allow &lt;folder&gt;</code>). Leave empty to use its first allowed folder. Agents you put on another computer use their own folder there.</p>
    {locked && <p className="reason">Stop the team to change its folder.</p>}
    <RunOn team={draft.id} />
  </section>;
}

function LocalFolder({ draft, edit, locked }: { draft: TeamDraft; edit: Edit; locked: boolean }) {
  const [picking, setPicking] = useState(false);
  const home = useHome();
  const overrides = draft.agents.filter(a => a.folder?.trim()).length;
  return <section className="fld" aria-label="Project folder">
    <h3 className="fld-label">Project folder</h3>
    <p className="folder-path">{draft.folder ? shortPath(draft.folder, home) : 'No project folder chosen'}</p>
    <p className="fld-hint muted">All agents use this folder unless you choose a different one for an agent.</p>
    <Btn disabled={locked} onClick={() => setPicking(true)}>Choose project folder…</Btn>
    {locked && <p className="reason">Stop the team to change its folder.</p>}
    {draft.folder && <Btn disabled={locked} onClick={() => edit(d => setProjectFolder(d, null))}>Clear project folder</Btn>}
    {draft.folder && overrides > 0 && <p className="fld-hint">{overrides} agent(s) use their own folders. <Btn disabled={locked} onClick={() => edit(inheritProjectFolder)}>Use project folder for all agents</Btn></p>}
    <OwnCopies team={draft.id} />
    {picking && <FolderPicker initialPath={draft.folder ?? null} onClose={() => setPicking(false)} onChoose={folder => {
      edit(d => setProjectFolder(d, folder)); setPicking(false);
    }} />}
  </section>;
}

/** Team setting: each Claude Code / Codex agent works in its own git copy (worktree) and branch. */
function OwnCopies({ team }: { team: string }) {
  const [s, setS] = useState<TeamSettings | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { api.teamSettings(team).then(setS, () => setS(null)); }, [team]);
  if (!s) return null;
  const toggle = async (on: boolean) => {
    setError('');
    try { setS(await api.saveTeamSettings(team, { ...s, own_copies: on })); } catch (e) { setError(errorText(e)); }
  };
  return <div className="own-copies">
    <label><input type="checkbox" checked={s.own_copies} onChange={e => void toggle(e.target.checked)} /> Each agent works in its own git copy</label>
    <p className="fld-hint muted">Agents running at the same time can't overwrite each other's files. Each gets a branch (sculptured/{team}/…) made from the last commit, so commit first; teammates see each other's work only after you merge it on the Git page. Needs the project folder to be a git repository. Applies when agents next start.</p>
    {error && <p className="tone-red" role="alert">{error}</p>}
  </div>;
}
