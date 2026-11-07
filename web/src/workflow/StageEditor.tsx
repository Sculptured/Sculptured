import {DismissibleError} from '../shell/DismissibleError';
import { useState } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Workflow } from '../generated/Workflow';
import type { WorkflowStage } from '../generated/WorkflowStage';
import { StageAdvanced } from './StageAdvanced';
import { typeOf } from './stageKinds';

type Update = (next: WorkflowStage) => void;

/** Who works on a stage in parallel: the responsible agent, and other agents taking separate topics. */
function WorkerFields({ draft, s, locked, update, onAddWorker }: { draft: TeamDraft; s: WorkflowStage; locked: boolean; update: Update; onAddWorker?: () => void }) {
  const w = draft.workflow!;
  const count = s.workers?.length ?? 0;
  return <><label>Responsible agent <select value={s.agent} disabled={locked} onChange={e => update({ ...s, agent: e.target.value, workers: (s.workers ?? []).filter(id => id !== e.target.value) })}>{draft.agents.map(a => <option value={a.id} key={a.id}>{a.name}</option>)}</select></label>
    <fieldset disabled={locked}><legend>Parallel topic workers · {1 + count} configured</legend>
      <p>Independent topics can use different workers at the same stage. Each worker handles one topic at a time.</p>
      {draft.agents.filter(a => a.id !== s.agent && a.id !== w.dispatcher && a.id !== w.collector).map(a => <label key={a.id}><input type="checkbox" disabled={count >= 7 && !s.workers?.includes(a.id)} checked={(s.workers ?? []).includes(a.id)} onChange={e => update({ ...s, workers: e.target.checked ? [...(s.workers ?? []), a.id] : (s.workers ?? []).filter(id => id !== a.id) })} />{a.name}</label>)}
      {onAddWorker && <button className="btn" disabled={count >= 7} onClick={onAddWorker}>Create parallel worker</button>}
    </fieldset>{locked && <p>Stop the team before changing worker capacity.</p>}</>;
}

function RouteRow({ w, s, k, to, locked, update }: { w: Workflow; s: WorkflowStage; k: string; to: string | null; locked: boolean; update: Update }) {
  return <div className="workflow-route"><label>When {k} → <select aria-label={`Destination for ${k}`} value={to ?? ''} disabled={locked} onChange={e => update({ ...s, routes: { ...s.routes, [k]: e.target.value || null } })}><option value="">Accept this item</option>{w.stages.map(n => <option key={n.id} value={n.id}>{n.name}</option>)}</select></label>
    <button className="link-btn" disabled={locked} onClick={() => { const routes = { ...s.routes }; delete routes[k]; update({ ...s, routes }); }}>Remove {k}</button></div>;
}

const OUTCOME = /^[a-z][a-z0-9-]*$/;

function NewOutcome({ s, locked, update }: { s: WorkflowStage; locked: boolean; update: Update }) {
  const [outcome, setOutcome] = useState('');
  const refused = !OUTCOME.test(outcome) || outcome === 'blocked' || outcome in s.routes || Object.keys(s.routes).length >= 8;
  return <><label>New outcome <input value={outcome} disabled={locked} placeholder="e.g. needs-example" onChange={e => setOutcome(e.target.value)} /></label>
    <button className="btn" disabled={locked || refused} onClick={() => { update({ ...s, routes: { ...s.routes, [outcome]: null } }); setOutcome(''); }}>Add branch</button></>;
}

export function StageEditor({ draft, stageId, locked, change, onOpen, onAddWorker }: { draft: TeamDraft; stageId: string; locked: boolean; change: (w: Workflow) => void; onOpen: (id: string) => void; onAddWorker?: () => void }) {
  const w = draft.workflow!, s = w.stages.find(s => s.id === stageId);
  if (!s) return <DismissibleError>This stage is missing. Choose another stage and fix its route.</DismissibleError>;
  const update: Update = (next) => change({ ...w, stages: w.stages.map(x => x.id === s.id ? next : x) });
  const type = typeOf(s);
  return <section className="workflow-editor" aria-label={`Stage ${s.name}`}>
    <h3>{s.name} {s.agent && <button className="link-btn" onClick={() => onOpen(s.agent)}>Open agent instructions</button>}</h3>
    <label>Stage name <input value={s.name} disabled={locked} onChange={e => update({ ...s, name: e.target.value })} /></label>
    {type === 'agent' && <WorkerFields draft={draft} s={s} locked={locked} update={update} onAddWorker={onAddWorker} />}
    {Object.entries(s.routes).map(([key, to]) => <RouteRow key={key} w={w} s={s} k={key} to={to} locked={locked} update={update} />)}
    {!['check', 'merge'].includes(type) && <NewOutcome s={s} locked={locked} update={update} />}
    <StageAdvanced draft={draft} stage={s} locked={locked} update={update} change={change} />
    <button className="btn" disabled={locked} onClick={() => change({ ...w, stages: w.stages.filter(n => n.id !== s.id) })}>Remove stage</button>
    <p>Loops return the same item and its context to the selected stage. The step limit bounds repeated reviews. Fix any routes that name a removed stage before starting.</p>
  </section>;
}
