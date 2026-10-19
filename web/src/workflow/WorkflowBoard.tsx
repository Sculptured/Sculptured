import { TextbookSetup } from './TextbookSetup';
import { useState } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Workflow } from '../generated/Workflow';
import { SchedulesEditor } from './SchedulesEditor';
import { TextArea } from '../shell/TextArea';
import { StageEditor } from './StageEditor';
import { WorkflowRunView } from './WorkflowRunView';
import './workflow.css';
import { addParallelWorker } from './workers';
import { byCommand, DispatchCommandFields } from './DispatchCommand';

type Props = { draft: TeamDraft; locked: boolean; edit?: (fn: (d: TeamDraft) => TeamDraft) => void; onOpen: (id: string) => void };

export function WorkflowBoard({ draft, locked, edit, onOpen }: Props) {
  const [selected, setSelected] = useState<string | null>(null);
  const w = draft.workflow!;
  const change = (next: Workflow) => edit?.(d => ({ ...d, workflow: next }));
  const name = (id: string) => draft.agents.find(a => a.id === id)?.name || 'Missing agent';
  return <div className="workflow-board" aria-label="Dynamic workflow">
    <section className="workflow-input"><h2>Input</h2><p>Drop links, notes, ideas, questions and teaching goals here. The dispatcher uses this to choose meaningful sections and the number of interactions.</p>
      <TextArea aria-label="Workflow input" value={w.input} rows={5} readOnly={locked || !edit} maxLength={64000} onChange={e => change({ ...w, input: e.target.value })} placeholder="Course links or local notes… Who is this for? What should the learner explore? Any constraints or ideas?" />
      <small>{locked ? 'Stop the project before changing the input for a new batch.' : 'Saved with this project. No generation starts until you start the team.'}</small>
      <TextbookSetup draft={draft} locked={locked || !edit} edit={edit} />
    </section>
    <section className="workflow-dispatch">{byCommand(w) ? <h3>↓ Dispatch command <code>$ {w.dispatch!.command}</code></h3> : <h3>↓ {name(w.dispatcher)} <button className="link-btn" onClick={() => onOpen(w.dispatcher)}>Open agent</button></h3>}
      <p><strong>Dynamically dispatches N interactions</strong> · 1–{w.max_tasks} items · up to {w.max_steps} steps per item · shared agents, one assignment per agent at a time.</p></section>
    <ol className="workflow-stages" aria-label="Reusable interaction stages">{w.stages.map(s => <li key={s.id} className={selected === s.id ? 'selected' : ''}>
      <button className="workflow-node" aria-label={`${s.id === w.entry ? "Entry stage" : "Reusable stage"} ${s.name} · ${name(s.agent)}`} onClick={() => setSelected(s.id)} aria-pressed={selected === s.id}><small>{s.id === w.entry ? 'Entry stage' : 'Reusable stage'}</small><strong>{s.name}</strong><span>{name(s.agent)}</span></button>
      <ul>{Object.entries(s.routes).map(([outcome, to]) => <li key={outcome}><span>{outcome} → </span>{to ? <button className="link-btn" onClick={() => setSelected(to)}>{w.stages.find(n => n.id === to)?.name || 'Missing stage'}</button> : <span>Item accepted</span>}</li>)}</ul>
    </li>)}</ol>
    {selected && <StageEditor key={selected} draft={draft} stageId={selected} locked={locked || !edit} change={change} onOpen={onOpen} onAddWorker={edit ? () => edit(d => addParallelWorker(d,selected)) : undefined} />}
    <section className="workflow-assembly"><h3>All N items accepted → {name(w.collector)} <button className="link-btn" onClick={() => onOpen(w.collector)}>Open agent</button></h3><p>Assemble the textbook, then run integration checks. Review acceptance is agent-reported; inspect the evidence below.</p></section>
    {edit && <WorkflowSettings draft={draft} locked={locked} change={change} editTeam={edit} />}
    <WorkflowRunView draft={draft} />
  </div>;
}

export function WorkflowSettings({ draft, locked, change, expanded, editTeam }: { draft: TeamDraft; locked: boolean; change: (w: Workflow) => void; expanded?: boolean; editTeam?: (fn: (d: TeamDraft) => TeamDraft) => void }) {
  const w = draft.workflow!;
  const none = (key: 'dispatcher' | 'collector') => key === 'dispatcher' && (!w.dispatcher || !!w.dispatch);
  const agent = (key: 'dispatcher' | 'collector') => <select aria-label={key === 'dispatcher' ? 'Dispatcher agent' : 'Assembly agent'} value={w[key]} disabled={locked || (key === 'dispatcher' && !!w.dispatch)} onChange={e => change({ ...w, [key]: e.target.value })}>
    {none(key) && <option value="">{w.dispatch ? 'None: the dispatch command makes the work' : 'Choose an agent'}</option>}{draft.agents.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select>;
  const add = () => { let n = 1; while (w.stages.some(s => s.id === `stage-${n}`)) n++; const id = `stage-${n}`; change({ ...w, stages: [...w.stages, { id, name: 'New stage', agent: draft.agents.find(a => a.id !== w.dispatcher && a.id !== w.collector)?.id || '', routes: { done: null } }] }); };
  return <details className="workflow-settings" open={expanded}><summary>Workflow settings & composition</summary><div>
    <label>Dispatcher {agent('dispatcher')}</label>{w.dispatch && <span className="reason">The dispatch command (Advanced) makes the work, so no agent dispatches it.</span>}<label>Assembly {agent('collector')}</label>
    <label>First stage <select value={w.entry} disabled={locked} onChange={e => change({ ...w, entry: e.target.value })}>{w.stages.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
    <label>Maximum items <input type="number" min={1} max={64} value={w.max_tasks} disabled={locked} onChange={e => change({ ...w, max_tasks: Number(e.target.value) })} /></label>
    <label>Maximum steps per item <input type="number" min={1} max={32} value={w.max_steps} disabled={locked} onChange={e => change({ ...w, max_steps: Number(e.target.value) })} /></label>
    <button className="btn" disabled={locked || w.stages.length >= 16} onClick={add}>Add reusable stage</button>
    <details className="workflow-settings-advanced" data-advanced open={!!w.dispatch || undefined}><summary>Advanced</summary>
      <DispatchCommandFields w={w} locked={locked} change={change} />
      <label><input type="checkbox" disabled={locked} checked={w.copies === 'per-item'} onChange={e => change({ ...w, copies: e.target.checked ? 'per-item' : undefined })} />Give each item its own copy and branch</label>
      <p className="muted">Each item gets its own git branch and folder in the project's repository, used by its agents, checks and merge stage. Needs a project folder with git.</p>
      <label>Guard command <input value={w.guard?.command ?? ''} disabled={locked} placeholder="e.g. npm test" onChange={e => change({ ...w, guard: e.target.value ? { ...w.guard, command: e.target.value, every_minutes: w.guard?.every_minutes ?? 15 } : undefined })} /></label>
      {w.guard && <label>Guard time limit in seconds <input type="number" min={1} max={3600} placeholder="600" value={w.guard.timeout_s ?? ''} disabled={locked} onChange={e => change({ ...w, guard: { ...w.guard!, timeout_s: e.target.value ? Number(e.target.value) : undefined } })} /></label>}
      {w.guard && <label>Run the guard every (minutes) <input type="number" min={5} max={1440} value={w.guard.every_minutes} disabled={locked} onChange={e => change({ ...w, guard: { ...w.guard!, every_minutes: Number(e.target.value) } })} /></label>}
      <p className="muted">While the guard fails, no new work goes out; it resumes when it passes or a person resumes.</p>
      {editTeam && <>
        <label><input type="checkbox" disabled={locked} checked={!!draft.decision_log} onChange={e => editTeam(d => ({ ...d, decision_log: e.target.checked || undefined }))} />Keep a decision log</label>
        <SchedulesEditor draft={draft} locked={locked} editTeam={editTeam} />
      </>}
    </details>
    <p>Select any stage to assign an agent or edit its outcome branches. Several stages can reuse one agent. Every stage can report blocked; no branch runs on a plain chat reply.</p>
  </div></details>;
}
