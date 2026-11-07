import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, newRequestId } from '../api';
import { errorText } from '../errors';
import type { GroupBlock } from '../generated/GroupBlock';
import type { TeamSummary } from '../generated/TeamSummary';
import type { TeamRecord } from '../generated/TeamRecord';
import { insertBlock } from './blocks';
import { DismissibleError } from '../shell/DismissibleError';
import { BlockFeatures, BlockDetails } from './BlockDetails';

type Pending = { team: TeamRecord; draft: TeamRecord['draft']; request: string };

/** Loads the projects a block can be added to, skipping trashed ones. */
function useDestinations(setError: (message: string) => void) {
  const [teams, setTeams] = useState<TeamSummary[]>([]);
  useEffect(() => {
    Promise.all([api.listTeams(), api.projectVisibility()]).then(([list, visibility]) => setTeams(list.filter(t => !visibility.some(v => v.id === t.id && v.status === 'trashed'))), e => setError(errorText(e)));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return teams;
}

/** The add flow survives a failed save: the created project, its name and the unsaved draft are kept for a retry. */
function useAddFlow(block: GroupBlock, destination: string, name: string, openProject: (id: string) => void) {
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const created = useRef<TeamRecord | null>(null), requestId = useRef(newRequestId());
  const pendingSave = useRef<Pending | null>(null);
  const creationName = useRef<string | null>(null);
  const targetTeam = async () => {
    if (destination !== 'new') return api.getTeam(destination);
    created.current ??= await api.createTeam('blank', creationName.current ??= name.trim(), requestId.current);
    return created.current;
  };
  const attempt = async () => {
    const p = pendingSave.current;
    if (p) { await api.saveTeam(p.team.draft.id, p.team.draft.revision, p.draft, p.request); return openProject(p.team.draft.id); }
    const team = await targetTeam();
    if (team.live) return setError('Stop this project before adding agents, or choose a new project.');
    const result = insertBlock(team.draft, block);
    if (result.reason) return setError(result.reason);
    const request = newRequestId();
    pendingSave.current = { team, draft: result.draft, request };
    await api.saveTeam(team.draft.id, team.draft.revision, result.draft, request);
    openProject(team.draft.id);
  };
  const add = async () => {
    if (busy) return;
    setBusy(true); setError('');
    try { await attempt(); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const started = !!created.current || !!pendingSave.current || !!creationName.current;
  return { error, setError, busy, add, started };
}

function DestinationForm({ block, teams, destination, setDestination, name, setName, flow }: { block: GroupBlock; teams: TeamSummary[]; destination: string; setDestination: (v: string) => void; name: string; setName: (v: string) => void; flow: ReturnType<typeof useAddFlow> }) {
  const locked = flow.busy || flow.started;
  return <form onSubmit={e => { e.preventDefault(); void flow.add(); }}>
    <label>Destination project<select value={destination} disabled={locked} onChange={e => setDestination(e.target.value)}><option value="new">Create a new project</option>{teams.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}</select></label>
    {destination === 'new' && <label>Project name<input required maxLength={150} value={name} disabled={locked} onChange={e => setName(e.target.value)} /></label>}
    <p className="muted">Adds an independent copy and opens its board. Review instructions and choose a folder before starting agents.</p>
    {block.boundary_notes.length > 0 && <details><summary>Connections to reconnect</summary><ul>{block.boundary_notes.map((n, i) => <li key={i}>{n}</li>)}</ul></details>}
    {flow.error && <DismissibleError>{flow.error}</DismissibleError>}
    <button className="btn btn-primary" disabled={flow.busy || (destination === 'new' && !name.trim())}>{flow.busy ? 'Adding…' : 'Add & open project'}</button>
  </form>;
}

export function AddToProject({ block, onClose }: { block: GroupBlock; onClose: () => void }) {
  const navigate = useNavigate(), dialog = useRef<HTMLDialogElement>(null);
  const [destination, setDestination] = useState('new'), [name, setName] = useState(block.name);
  const openProject = (id: string) => { window.dispatchEvent(new Event('projects-changed')); navigate(`/team/${encodeURIComponent(id)}/structure`); onClose(); };
  const flow = useAddFlow(block, destination, name, openProject);
  const teams = useDestinations(flow.setError);
  useEffect(() => { const previous = document.activeElement as HTMLElement | null; dialog.current?.showModal(); return () => previous?.focus(); }, []);
  return <dialog ref={dialog} className="block-modal project-picker" aria-label={`Add ${block.name} to project`} onCancel={e => { if (flow.busy) e.preventDefault(); else onClose(); }}>
    <header><h2>Add {block.name}</h2><button className="btn" disabled={flow.busy} onClick={onClose}>Cancel</button></header>
    <BlockFeatures block={block} /><details><summary>Inspect this block</summary><BlockDetails block={block} /></details>
    <DestinationForm block={block} teams={teams} destination={destination} setDestination={setDestination} name={name} setName={setName} flow={flow} />
  </dialog>;
}
