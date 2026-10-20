import { useRef, useState } from 'react';
import type { Component } from '../generated/Component';
import type { TeamDraft } from '../generated/TeamDraft';
import { Btn } from '../shell/ui';
import { communityRequest, useCommunitySession } from '../community/share';
import { ShareSignIn } from '../community/ShareSignIn';
import type { Edit } from './Panels';
import type { ComponentKind } from '../generated/ComponentKind';
import { BUILTIN, KIND_LABEL, componentProblems, parseManifest, whereItRuns } from './components';
import './components.css';
import { McpRegistrySearch } from './McpRegistrySearch';

/** Share one of the team's components to Community, as its own block. */
function ShareButton({ c }: { c: Component }) {
  const { session } = useCommunitySession();
  const [state, setState] = useState<string | null>(null);
  if (!session.user) return <ShareSignIn />;
  const share = async () => {
    setState('Sharing…');
    try {
      await communityRequest('blocks', session, 'POST', { title: c.name, description: c.description, license: 'Apache-2.0', tags: ['component'], content: { kind: 'component', component: c }, reviewed: true });
      setState('Shared to Community');
    } catch (e) {
      setState(e instanceof Error ? e.message : 'Could not share it');
    }
  };
  return <>{state ? <span className="muted">{state}</span> : <Btn size="sm" onClick={() => void share()}>Share to Community</Btn>}</>;
}

type Form = { kind: ComponentKind; name: string; description: string; install: string; url: string; headers: [string, string][]; program: string; args: string; plugin: string; skillsFrom: string };
const BLANK: Form = { kind: 'mcp-remote', name: '', description: '', install: '', url: '', headers: [['Authorization', 'Bearer ${API_TOKEN}']], program: '', args: '', plugin: '', skillsFrom: '' };
const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40).replace(/-+$/, '');

/** The manifest a filled-in form describes. */
function fromForm(f: Form, draft: TeamDraft): Component {
  const c: Component = { id: slug(f.name) || 'component', name: f.name.trim(), description: f.description.trim(), kind: f.kind };
  if (f.kind !== 'skills') c.install = f.install.trim();
  if (f.kind === 'mcp-remote') {
    c.url = f.url.trim();
    const headers = Object.fromEntries(f.headers.filter(([k]) => k.trim()).map(([k, v]) => [k.trim(), v.trim()]));
    if (Object.keys(headers).length) c.headers = headers;
  }
  if (f.kind === 'mcp') {
    c.program = f.program.trim();
    const args = f.args.trim().split(/\s+/).filter(Boolean);
    if (args.length) c.args = args;
  }
  if (f.kind === 'claude-plugin') { c.plugin = f.plugin.trim(); c.claude_only = true; }
  if (f.kind === 'skills') c.skills = draft.agents.find((a) => a.id === f.skillsFrom)?.advanced.skills ?? [];
  return c;
}

/** A remote server's headers, each a name and a value that names its secret as ${NAME}. */
function HeaderFields({ headers, onChange }: { headers: [string, string][]; onChange: (h: [string, string][]) => void }) {
  const change = (i: number, at: 0 | 1, v: string) => onChange(headers.map((h, j) => (j === i ? (at === 0 ? [v, h[1]] : [h[0], v]) : h)));
  return (
    <fieldset className="component-headers"><legend className="fld-label">Headers</legend>
      <p className="fld-hint muted">Write a secret as {'${NAME}'}. It stays on each agent's computer, where its owner sets it with <code>sculptured set-secret NAME</code>; the team and Community never see it.</p>
      {headers.map(([k, v], i) => (
        <span key={i} className="row">
          <input className="fld-input" aria-label={`Header ${i + 1} name`} value={k} onChange={(e) => change(i, 0, e.target.value)} />
          <input className="fld-input mono" aria-label={`Header ${i + 1} value`} value={v} onChange={(e) => change(i, 1, e.target.value)} />
          <Btn size="sm" aria-label={`Remove header ${i + 1}`} onClick={() => onChange(headers.filter((_, j) => j !== i))}>Remove</Btn>
        </span>
      ))}
      <Btn size="sm" onClick={() => onChange([...headers, ['', '']])}>Add header</Btn>
    </fieldset>
  );
}

/** Pick an agent whose skills become the pack. */
function SkillsFrom({ draft, value, onChange }: { draft: TeamDraft; value: string; onChange: (id: string) => void }) {
  const withSkills = draft.agents.filter((a) => a.advanced.skills?.length);
  return (
    <label className="fld"><span>Skills from</span>
      <select className="fld-input" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{withSkills.length ? 'Choose an agent…' : 'No agent has skills yet'}</option>
        {withSkills.map((a) => <option key={a.id} value={a.id}>{a.name} ({a.advanced.skills!.length})</option>)}
      </select></label>
  );
}

const SETUP_HINT: Partial<Record<ComponentKind, string>> = { 'claude-plugin': 'claude plugin install openfoam@hpc-tools', 'mcp-remote': 'Ask the HPC team for a token', mcp: 'pip install …' };

/** Make a component from a short form: a remote MCP server (such as a hosted OpenFOAM server), one on
 * the computer, an installed Claude plugin, or a pack of an agent's skills. */
function NewComponent({ draft, team, initial, onAdd, onClose }: { draft: TeamDraft; team: Component[]; initial?: Partial<Form>; onAdd: (c: Component) => void; onClose: () => void }) {
  const [f, setF] = useState<Form>({ ...BLANK, ...initial });
  const [problems, setProblems] = useState<string[]>([]);
  const set = (patch: Partial<Form>) => setF((x) => ({ ...x, ...patch }));
  const text = (label: string, key: 'name' | 'description' | 'install' | 'url' | 'program' | 'args' | 'plugin', placeholder = '') => (
    <label className="fld"><span>{label}</span><input className="fld-input" value={f[key]} placeholder={placeholder} onChange={(e) => set({ [key]: e.target.value })} /></label>
  );
  const add = () => {
    const c = fromForm(f, draft);
    const found = componentProblems(c, team);
    setProblems(found);
    if (found.length === 0) { onAdd(c); onClose(); }
  };
  return (
    <div className="component-new" role="group" aria-label="New component">
      <label className="fld"><span>Kind</span>
        <select className="fld-input" value={f.kind} onChange={(e) => set({ kind: e.target.value as ComponentKind })}>
          {(['mcp-remote', 'mcp', 'claude-plugin', 'skills'] as ComponentKind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
        </select></label>
      {text('Name', 'name', f.kind === 'mcp-remote' ? 'OpenFOAM' : '')}
      {text('What it gives an agent', 'description')}
      {f.kind === 'mcp-remote' && <>{text('Address', 'url', 'https://…/mcp')}<HeaderFields headers={f.headers} onChange={(headers) => set({ headers })} /></>}
      {f.kind === 'mcp' ? <>{text('Program', 'program', 'kin-mcp')}{text('Arguments', 'args')}</> : f.kind === 'claude-plugin' && text('Plugin (name@marketplace)', 'plugin', 'openfoam@hpc-tools')}
      {f.kind === 'skills' && <SkillsFrom draft={draft} value={f.skillsFrom} onChange={(skillsFrom) => set({ skillsFrom })} />}
      {f.kind !== 'skills' && text('How to set it up on a computer', 'install', SETUP_HINT[f.kind])}
      <div className="fld-actions"><Btn size="sm" onClick={add}>Add</Btn><Btn size="sm" onClick={onClose}>Cancel</Btn></div>
      {problems.length > 0 && <ul role="alert" className="fld-error">{problems.map((p) => <li key={p}>{p}</li>)}</ul>}
    </div>
  );
}

/** Add a component from a manifest: pasted, or loaded from a file (such as one downloaded from Community). */
function AddComponent({ team, onAdd }: { team: Component[]; onAdd: (c: Component) => void }) {
  const [text, setText] = useState('');
  const [problems, setProblems] = useState<string[]>([]);
  const file = useRef<HTMLInputElement>(null);
  const add = (raw: string) => {
    const c = parseManifest(raw);
    const found = typeof c === 'string' ? [c] : componentProblems(c, team);
    setProblems(found);
    if (typeof c !== 'string' && found.length === 0) { onAdd(c); setText(''); }
  };
  return (
    <div className="component-add">
      <label className="fld"><span>Add a component: paste its manifest (JSON)</span>
        <textarea className="fld-input mono" rows={4} value={text} onChange={(e) => setText(e.target.value)} placeholder='{"id": "…", "name": "…", "kind": "mcp", "program": "…", "install": "…"}' /></label>
      <div className="fld-actions">
        <Btn size="sm" disabled={!text.trim()} onClick={() => add(text)}>Add component</Btn>
        <Btn size="sm" onClick={() => file.current?.click()}>Load from file</Btn>
        <input ref={file} type="file" accept=".json,application/json" hidden aria-label="Component manifest file" onChange={(e) => { const f = e.target.files?.[0]; if (f) void f.text().then(add); e.target.value = ''; }} />
      </div>
      {problems.length > 0 && <ul role="alert" className="fld-error">{problems.map((p) => <li key={p}>{p}</li>)}</ul>}
    </div>
  );
}

/** The team's components (Advanced): Sculptured's built-in ones, and the team's own, each with exactly
 * what it runs or connects to. On a connected computer, one that is not built in runs only after the computer's
 * owner approves it there. */
export function ComponentsSection({ draft, edit, locked }: { draft: TeamDraft; edit: Edit; locked: boolean }) {
  const own = draft.components ?? [];
  const [making, setMaking] = useState<Partial<Form> | null>(null);
  const [searching, setSearching] = useState(false);
  const set = (next: Component[]) => edit((d: TeamDraft) => ({ ...d, components: next.length ? next : undefined }), 'components');
  const row = (c: Component, builtin: boolean) => (
    <li key={c.id}>
      <span className="row"><b>{c.name}</b>{builtin ? <span className="muted">built in</span> : (
        <span className="fld-actions"><ShareButton c={c} /><Btn size="sm" disabled={locked} aria-label={`Remove component ${c.name}`} onClick={() => set(own.filter((o) => o.id !== c.id))}>Remove</Btn></span>
      )}</span>
      <span className="muted">{c.description}</span>
      <span className="muted">{KIND_LABEL[c.kind]} · <code>{whereItRuns(c)}</code>{c.install ? <> · set up: {c.install}</> : null}</span>
    </li>
  );
  return (
    <fieldset className="fld-skills" aria-label="Components">
      <legend className="fld-label">Components</legend>
      <p className="fld-hint muted">What agents can be given besides Sculptured's tools: MCP servers on their computers or at a web address, Claude plugins and tool hooks, and skill packs. Give one to an agent in its Advanced settings. Find more in <a href="/community">Community</a>, or add your own; a connected computer runs one that is not built in only after its owner approves it there (<code>sculptured approve-component</code>).</p>
      <ul className="components-list">{BUILTIN.map((c) => row(c, true))}{own.map((c) => row(c, false))}</ul>
      {!locked && (making
        ? <NewComponent draft={draft} team={own} initial={making} onAdd={(c) => set([...own, c])} onClose={() => setMaking(null)} />
        : <div className="fld-actions"><Btn size="sm" icon="plus" onClick={() => setMaking({})}>New component</Btn><Btn size="sm" onClick={() => setSearching(true)}>From the MCP Registry</Btn></div>)}
      {!locked && searching && !making && <McpRegistrySearch onClose={() => setSearching(false)} onUse={(f) => { setSearching(false); setMaking(f); }} />}
      {!locked && <AddComponent team={own} onAdd={(c) => set([...own, c])} />}
    </fieldset>
  );
}
