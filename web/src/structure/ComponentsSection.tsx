import { useRef, useState } from 'react';
import type { Component } from '../generated/Component';
import type { TeamDraft } from '../generated/TeamDraft';
import { Btn } from '../shell/ui';
import { communityRequest, useCommunitySession } from '../release/Community';
import type { Edit } from './Panels';
import { BUILTIN, commandLine, componentProblems, parseManifest } from './components';
import './components.css';

/** Share one of the team's components to Community, as its own block. */
function ShareButton({ c }: { c: Component }) {
  const { session } = useCommunitySession();
  const [state, setState] = useState<string | null>(null);
  if (!session.user) return <a className="btn btn-sm" href="/api/community/login">Sign in to share</a>;
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

/** The team's components (Advanced): Sculptured's built-in ones, and the team's own, each with the exact
 * command it runs. On a connected computer, one that is not built in runs only after the computer's
 * owner approves it there. */
export function ComponentsSection({ draft, edit, locked }: { draft: TeamDraft; edit: Edit; locked: boolean }) {
  const own = draft.components ?? [];
  const set = (next: Component[]) => edit((d: TeamDraft) => ({ ...d, components: next.length ? next : undefined }), 'components');
  const row = (c: Component, builtin: boolean) => (
    <li key={c.id}>
      <span className="row"><b>{c.name}</b>{builtin ? <span className="muted">built in</span> : (
        <span className="fld-actions"><ShareButton c={c} /><Btn size="sm" disabled={locked} aria-label={`Remove component ${c.name}`} onClick={() => set(own.filter((o) => o.id !== c.id))}>Remove</Btn></span>
      )}</span>
      <span className="muted">{c.description}</span>
      <span className="muted">Runs <code>{commandLine(c)}</code> on the agent's computer · install: {c.install}</span>
    </li>
  );
  return (
    <fieldset className="fld-skills" aria-label="Components">
      <legend className="fld-label">Components</legend>
      <p className="fld-hint muted">Programs on the agents' computers that agents can also use. Give one to an agent in its Advanced settings. Find more in Community, or add your own; a connected computer runs one that is not built in only after its owner approves it there (<code>sculptured approve-component</code>).</p>
      <ul className="components-list">{BUILTIN.map((c) => row(c, true))}{own.map((c) => row(c, false))}</ul>
      {!locked && <AddComponent team={own} onAdd={(c) => set([...own, c])} />}
    </fieldset>
  );
}
