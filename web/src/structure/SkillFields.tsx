import { useEffect, useRef, useState } from 'react';
import type { DraftAgent } from '../generated/DraftAgent';
import type { Component } from '../generated/Component';
import { KIND_GROUPS, availableComponents, componentById, secretsOf } from './components';
import type { Skill } from '../generated/Skill';
import type { TeamDraft } from '../generated/TeamDraft';
import { setAdvanced } from '../draft/edits';
import { Btn } from '../shell/ui';
import '../shell/text-mode.css';
import type { Edit } from './Panels';
import { parseSkillMd, skillProblems } from './skills';
import { SkillCatalog } from './SkillCatalog';

const EMPTY: Skill = { name: '', description: '', body: '' };

/** The full-size window where one skill is written or changed. */
function SkillEditor({ initial, others, onSave, onClose }: { initial: Skill; others: Skill[]; onSave: (s: Skill) => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [s, setS] = useState(initial);
  const [tried, setTried] = useState(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const problems = skillProblems(s, others);
  const save = () => { setTried(true); if (problems.length === 0) onSave({ ...s, description: s.description.trim() }); };
  return (
    <dialog ref={dialog} className="text-mode skill-editor" aria-label={initial.name ? `Skill ${initial.name}` : 'New skill'} onKeyDown={(e) => e.stopPropagation()} onCancel={(e) => { e.preventDefault(); onClose(); }}>
      <header>
        <div><h2>{initial.name ? `Skill: ${initial.name}` : 'New skill'}</h2><small>Only the name and description stay in the agent's context until it uses the skill.</small></div>
        <div className="fld-actions"><Btn onClick={onClose}>Cancel</Btn><Btn variant="primary" onClick={save}>Save skill</Btn></div>
      </header>
      <label className="fld"><span>Name</span>
        <input className="fld-input mono" value={s.name} placeholder="money-rules" maxLength={64} onChange={(e) => setS({ ...s, name: e.target.value })} />
      </label>
      <label className="fld"><span>When to use it</span>
        <input className="fld-input" value={s.description} placeholder="Use when handling amounts of money." maxLength={1024} onChange={(e) => setS({ ...s, description: e.target.value })} />
      </label>
      <label className="fld skill-body"><span>Instructions (Markdown)</span>
        <textarea className="text-mode-editor" value={s.body} onChange={(e) => setS({ ...s, body: e.target.value })} />
      </label>
      {tried && problems.length > 0 && <ul role="alert" className="fld-error">{problems.map((p) => <li key={p}>{p}</li>)}</ul>}
    </dialog>
  );
}

/** Where a skill was copied from: a GitHub repository by name, any other address as it is. */
function SourceLink({ url }: { url: string }) {
  const repo = /^https:\/\/github\.com\/([^/]+\/[^/]+)\//.exec(url)?.[1];
  return <span className="muted"> · from <a href={url} target="_blank" rel="noreferrer">{repo ?? url}</a></span>;
}

function SkillList({ skills, locked, onEdit, onRemove }: { skills: Skill[]; locked: boolean; onEdit: (at: number) => void; onRemove: (at: number) => void }) {
  if (skills.length === 0) return <p className="fld-hint muted">No skills. A skill is instructions the agent loads only when it needs them, so they cost nothing until then.</p>;
  return (
    <ul className="skill-list">
      {skills.map((s, i) => (
        <li key={s.name}>
          <span><code>{s.name}</code> <span className="muted">{s.description}</span>{s.source && <SourceLink url={s.source} />}</span>
          <span className="fld-actions">
            <Btn size="sm" disabled={locked} onClick={() => onEdit(i)}>Edit</Btn>
            <Btn size="sm" disabled={locked} aria-label={`Remove skill ${s.name}`} onClick={() => onRemove(i)}>Remove</Btn>
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Load from file: a SKILL.md split into a skill, which then opens in the editor. */
function useSkillFile(onLoaded: (s: Skill) => void) {
  const [error, setError] = useState<string | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const load = async (f: File | undefined) => {
    if (!f) return;
    const parsed = parseSkillMd(await f.text());
    setError(typeof parsed === 'string' ? `${f.name}: ${parsed}` : null);
    if (typeof parsed !== 'string') onLoaded(parsed);
  };
  const input = <input ref={file} type="file" accept=".md,text/markdown" hidden aria-label="SKILL.md file" onChange={(e) => { void load(e.target.files?.[0]); e.target.value = ''; }} />;
  return { error, input, open: () => file.current?.click() };
}

/** The agent's skills: a list, Add skill, Load from file. Skills belong to this agent only. */
export function SkillFields({ agent, edit, locked }: { agent: DraftAgent; edit: Edit; locked: boolean }) {
  const skills = agent.advanced.skills ?? [];
  const [editing, setEditing] = useState<{ at: number; skill: Skill } | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const picker = useSkillFile((skill) => setEditing({ at: -1, skill }));
  const set = (next: Skill[]) => edit((d: TeamDraft) => setAdvanced(d, agent.id, { skills: next.length ? next : undefined }), `skills:${agent.id}`);
  const save = (s: Skill) => {
    const next = [...skills];
    if (editing && editing.at >= 0) next[editing.at] = s; else next.push(s);
    set(next);
    setEditing(null);
  };
  return (
    <fieldset className="fld-skills">
      <legend className="fld-label">Skills</legend>
      <SkillList skills={skills} locked={locked} onEdit={(at) => setEditing({ at, skill: skills[at] })} onRemove={(at) => set(skills.filter((_, j) => j !== at))} />
      <div className="fld-actions">
        <Btn size="sm" icon="plus" disabled={locked} onClick={() => setEditing({ at: -1, skill: EMPTY })}>Add skill</Btn>
        <Btn size="sm" disabled={locked} onClick={() => setBrowsing(true)}>Add from a catalog</Btn>
        <Btn size="sm" disabled={locked} onClick={picker.open}>Load from file</Btn>
        {picker.input}
      </div>
      {picker.error && <p role="alert" className="fld-error">{picker.error}</p>}
      {browsing && <SkillCatalog onClose={() => setBrowsing(false)} onAdd={(skill) => { setBrowsing(false); setEditing({ at: -1, skill }); }} />}
      {editing && <SkillEditor initial={editing.skill} others={skills.filter((_, j) => j !== editing.at)} onSave={save} onClose={() => setEditing(null)} />}
    </fieldset>
  );
}

/** One component's line in the picker: what it gives, what it brings, and how to set it up. */
function hint(c: Component, d: TeamDraft | null | undefined): string {
  const brings = (c.requires ?? []).map((r) => componentById(d, r)?.name ?? r);
  const secrets = secretsOf(c);
  return [
    c.description,
    brings.length ? `Brings ${brings.join(', ')}.` : '',
    c.kind === 'skills' ? `Skills: ${(c.skills ?? []).map((s) => s.name).join(', ')}.` : '',
    secrets.length ? `Needs ${secrets.join(', ')} set on its computer.` : '',
    c.install ? `Set up on its computer: ${c.install}.` : '',
  ].filter(Boolean).join(' ');
}

/** One section of the picker, such as MCP servers. */
function PickerGroup({ title, items, agent, have, locked, draft, onToggle }: { title: string; items: Component[]; agent: DraftAgent; have: string[]; locked: boolean; draft?: TeamDraft | null; onToggle: (c: Component, on: boolean) => void }) {
  return (
    <fieldset className="picker-group" aria-label={title}>
      <legend className="fld-label">{title}</legend>
      {items.map((c) => {
        const claudeOnly = Boolean(c.claude_only) && agent.kind !== 'claude';
        return (
          <label key={c.id} className="fld fld-check">
            <span><input type="checkbox" checked={have.includes(c.id) && !claudeOnly} disabled={locked || claudeOnly} onChange={(e) => onToggle(c, e.target.checked)} /> {c.name}{claudeOnly && <span className="muted"> · Claude Code agents only</span>}</span>
            <span className="fld-hint muted">{hint(c, draft)}</span>
          </label>
        );
      })}
    </fieldset>
  );
}

/** The components an agent is given (Advanced), in sections: MCP servers, skill packs, and plugins and
 * hooks, with a search once there are many. Claude-only ones are offered to Claude agents only. Without
 * one set up on its computer, the agent starts anyway and its page says what to install. */
export function IntegrationFields({ agent, edit, locked, draft }: { agent: DraftAgent; edit: Edit; locked: boolean; draft?: TeamDraft | null }) {
  const have = agent.advanced.integrations ?? [];
  const [query, setQuery] = useState('');
  const all = availableComponents(draft);
  const toggle = (c: Component, checked: boolean) => edit((d: TeamDraft) => {
    let next = have.filter((i) => i !== c.id);
    if (checked) next = [...next, ...(c.requires ?? []).filter((r) => !next.includes(r)), c.id];
    // Turning off something another one requires turns that one off too.
    if (!checked) next = next.filter((i) => !(componentById(d, i)?.requires ?? []).includes(c.id));
    return setAdvanced(d, agent.id, { integrations: next.length ? next : undefined });
  }, `integrations:${agent.id}`);
  const q = query.trim().toLowerCase();
  const shown = all.filter((c) => !q || `${c.name} ${c.description} ${c.id}`.toLowerCase().includes(q));
  return (
    <div className="component-picker">
      {all.length > 6 && <input type="search" className="fld-input picker-search" aria-label="Find a component" placeholder="Find…" value={query} onChange={(e) => setQuery(e.target.value)} />}
      {KIND_GROUPS.map((g) => ({ g, items: shown.filter((c) => g.kinds.includes(c.kind)) })).filter(({ items }) => items.length).map(({ g, items }) => (
        <PickerGroup key={g.title} title={g.title} items={items} agent={agent} have={have} locked={locked} draft={draft} onToggle={toggle} />
      ))}
      {q && !shown.length && <p className="muted">Nothing here matches "{query}".</p>}
      <p className="fld-hint muted">Add MCP servers, plugins and skill packs to the team in Team settings → Advanced, or find them in <a href="/community">Community</a>.</p>
    </div>
  );
}
