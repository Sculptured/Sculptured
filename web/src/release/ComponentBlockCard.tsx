import { useState } from 'react';
import { api } from '../api';
import type { Component } from '../generated/Component';
import type { TeamSummary } from '../generated/TeamSummary';
import { commandLine } from '../structure/components';

export type ComponentBlock = { id: string; kind: 'component'; title: string; description: string; author: string; version: number; license: string; tags: string[]; content: { kind: 'component'; component: Component } };

/** Adds a component to one of the person's teams, unless that team has it already. */
async function addToTeam(team: string, c: Component): Promise<string> {
  const rec = await api.getTeam(team);
  if ((rec.draft.components ?? []).some((x) => x.id === c.id)) return 'That team has it already.';
  await api.saveTeam(team, rec.draft.revision, { ...rec.draft, components: [...(rec.draft.components ?? []), c] });
  return 'Added. Give it to agents in their Advanced settings.';
}

/** A component shared in Community: what it gives agents and exactly what it runs. */
export function ComponentBlockCard({ block }: { block: ComponentBlock }) {
  const c = block.content.component;
  const [teams, setTeams] = useState<TeamSummary[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const pick = async () => { try { setTeams(await api.listTeams()); } catch { setNote('Open Sculptured to add it to a team.'); } };
  return (
    <article className="release-card block-publication" aria-label={`Component ${block.title}`}>
      <div className="release-tags"><span>component</span>{block.tags.filter((t) => t !== 'component').map((t) => <span key={t}>{t}</span>)}</div>
      <h2>{block.title}</h2>
      <p>{c.description}</p>
      <p className="muted">Runs <code>{commandLine(c)}</code> on the agent's computer · install: {c.install}{c.homepage && <> · <a href={c.homepage} target="_blank" rel="noreferrer">homepage</a></>}</p>
      <small>By {block.author} · v{block.version} · {block.license}</small>
      <div className="release-actions">
        {teams ? (
          <select aria-label="Add to which team" defaultValue="" onChange={(e) => e.target.value && void addToTeam(e.target.value, c).then(setNote, () => setNote('Could not add it.'))}>
            <option value="">Add to which team…</option>
            {teams.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </select>
        ) : <button className="btn btn-primary" onClick={() => void pick()}>Add to a team</button>}
        <button className="btn" onClick={() => void navigator.clipboard?.writeText(JSON.stringify(c, null, 2)).then(() => setNote('Manifest copied.'))}>Copy manifest</button>
      </div>
      {note && <p className="muted" role="status">{note}</p>}
    </article>
  );
}
