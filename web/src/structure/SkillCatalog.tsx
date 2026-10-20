import { useEffect, useRef, useState } from 'react';
import type { Skill } from '../generated/Skill';
import { Btn } from '../shell/ui';
import { KNOWN_CATALOGS, loadCatalog, repoOf, type SkillCatalogRead } from './githubSkills';

/** Add from a catalog: a GitHub repository of Agent Skills, searched and previewed, one skill added at a time. */
export function SkillCatalog({ onAdd, onClose }: { onAdd: (s: Skill) => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [read, setRead] = useState<SkillCatalogRead | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => { dialog.current?.showModal(); }, []);
  const open = (repo: string | null) => {
    if (!repo) { setError('Give a GitHub repository as owner/name or its github.com address.'); return; }
    setBusy(true); setError(''); setRead(null);
    loadCatalog(repo).then(setRead, (e: Error) => setError(e.message)).finally(() => setBusy(false));
  };
  return (
    <dialog ref={dialog} className="text-mode skill-editor" aria-label="Add a skill from a catalog" onKeyDown={(e) => e.stopPropagation()} onCancel={(e) => { e.preventDefault(); onClose(); }}>
      <header>
        <div><h2>Add a skill from a catalog</h2><small>Read a skill before you add it: it becomes part of the agent's instructions. Only its SKILL.md text is added, not its scripts or other files.</small></div>
        <div className="fld-actions"><Btn onClick={onClose}>Close</Btn></div>
      </header>
      <CatalogPicker busy={busy} onOpen={open} />
      {busy && <p role="status" className="muted">Reading the catalog…</p>}
      {error && <p role="alert" className="fld-error">{error}</p>}
      {read && <CatalogList read={read} onAdd={onAdd} />}
    </dialog>
  );
}

/** Anthropic's or Google's catalog, or any other GitHub repository of skills. */
function CatalogPicker({ busy, onOpen }: { busy: boolean; onOpen: (repo: string | null) => void }) {
  const [choice, setChoice] = useState('');
  const [other, setOther] = useState('');
  return (
    <>
      <label className="fld"><span>Catalog</span>
        <select className="fld-input" value={choice} onChange={(e) => { setChoice(e.target.value); if (e.target.value && e.target.value !== 'other') onOpen(e.target.value); }}>
          <option value="">Choose a catalog</option>
          {KNOWN_CATALOGS.map((c) => <option key={c.repo} value={c.repo}>{c.name} ({c.repo})</option>)}
          <option value="other">Another GitHub repository…</option>
        </select>
      </label>
      {choice === 'other' && (
        <div className="fld">
          <label htmlFor="skill-catalog-repo">GitHub repository</label>
          <input id="skill-catalog-repo" className="fld-input mono" value={other} placeholder="owner/name or https://github.com/owner/name" onChange={(e) => setOther(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') onOpen(repoOf(other)); }} />
          <Btn onClick={() => onOpen(repoOf(other))} disabled={busy}>Open</Btn>
        </div>
      )}
    </>
  );
}

function CatalogList({ read, onAdd }: { read: SkillCatalogRead; onAdd: (s: Skill) => void }) {
  const [query, setQuery] = useState('');
  const [preview, setPreview] = useState<string | null>(null);
  const q = query.trim().toLowerCase();
  const shown = read.skills.filter((s) => !q || s.skill.name.includes(q) || s.skill.description.toLowerCase().includes(q));
  return (
    <section aria-label="Skills in this catalog">
      <p className="fld-hint muted">{read.skills.length} skills in {read.repo} at {read.commit.slice(0, 7)}{read.unreadable ? ` · ${read.unreadable} could not be read` : ''}</p>
      <label className="fld"><span>Search skills</span><input className="fld-input" value={query} onChange={(e) => setQuery(e.target.value)} /></label>
      <ul className="skill-list">
        {shown.map(({ path, skill }) => (
          <li key={path}>
            <div><b className="mono">{skill.name}</b> <span className="muted">{skill.description}</span></div>
            <div className="fld-actions">
              <Btn size="sm" aria-label={`Preview ${skill.name}`} onClick={() => setPreview(preview === path ? null : path)}>{preview === path ? 'Hide' : 'Preview'}</Btn>
              <Btn size="sm" aria-label={`Add ${skill.name}`} onClick={() => onAdd(skill)}>Add</Btn>
            </div>
            {preview === path && <pre className="text-mode-editor">{skill.body}</pre>}
          </li>
        ))}
      </ul>
      {shown.length === 0 && <p className="muted">No skill matches.</p>}
    </section>
  );
}
