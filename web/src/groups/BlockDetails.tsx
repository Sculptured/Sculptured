import { useState } from 'react';
import type { GroupBlock } from '../generated/GroupBlock';
import type { Selection } from '../draft/edits';
import { StructureCanvas } from '../structure/StructureCanvas';
import { Markdown } from '../shell/Markdown';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';
import '../shell/chat.css';
import './groups.css';

type Draft = GroupBlock['draft'];

const CONNECTION_TEXT: Record<string, string> = {
  'hands-work-to': 'Hands completed work to the recipient.',
  'reports-to': 'Reports to this agent. Coordination uses managed messages.',
};
const connectionText = (kind: string) => CONNECTION_TEXT[kind] ?? 'Watches this agent. Coordination uses managed messages.';

function ComponentList({ d, selection, target, select, pickStage }: { d: Draft; selection: Selection | null; target: WorkflowTarget | null; select: (s: Selection) => void; pickStage: (id: string) => void }) {
  const name = (id: string) => d.agents.find(a => a.id === id)?.name ?? id;
  const on = (kind: Selection['kind'], id: string) => selection?.kind === kind && selection.id === id;
  const stageId = target && typeof target === 'object' && 'stage' in target ? target.stage : null;
  return <nav className="block-component-list" aria-label="Block components">
    {d.agents.map(a => <button key={a.id} className="btn" aria-pressed={on('agent', a.id)} onClick={() => select({ kind: 'agent', id: a.id })}>{a.name}</button>)}
    {d.connections.map(c => <button key={c.id} className="btn" aria-pressed={on('connection', c.id)} onClick={() => select({ kind: 'connection', id: c.id })}>{name(c.from)} → {name(c.to)}</button>)}
    {d.workflow?.stages.map(s => <button key={s.id} className="btn" aria-pressed={stageId === s.id} onClick={() => pickStage(s.id)}>{s.name}</button>)}
  </nav>;
}

function StageDetail({ d, stage }: { d: Draft; stage: NonNullable<Draft['workflow']>['stages'][number] }) {
  const name = (id: string) => d.agents.find(a => a.id === id)?.name ?? id;
  const next = (to: string | null) => (to ? d.workflow?.stages.find(s => s.id === to)?.name ?? to : 'Complete');
  return <><h3>{stage.name}</h3><p>Responsible agent: {name(stage.agent)}</p><ul>{Object.entries(stage.routes).map(([outcome, to]) => <li key={outcome}>{outcome} → {next(to)}</li>)}</ul><details><summary>Full stage configuration</summary><pre>{JSON.stringify(stage, null, 2)}</pre></details></>;
}

function AgentDetail({ agent }: { agent: Draft['agents'][number] }) {
  return <><h3>{agent.name}</h3><p className="muted">{agent.kind} agent · inherits the project folder</p><ReadableInstructions text={agent.job} /><details><summary>Agent configuration</summary><pre>{JSON.stringify(agent.advanced, null, 2)}</pre></details></>;
}

function ConnectionDetail({ d, connection }: { d: Draft; connection: Draft['connections'][number] }) {
  const name = (id: string) => d.agents.find(a => a.id === id)?.name ?? id;
  return <><h3>{name(connection.from)} → {name(connection.to)}</h3><p>{connectionText(connection.kind)}</p></>;
}

function SelectionDetail({ d, selection }: { d: Draft; selection: Selection | null }) {
  const agent = selection?.kind === 'agent' ? d.agents.find(a => a.id === selection.id) : null;
  const connection = selection?.kind === 'connection' ? d.connections.find(c => c.id === selection.id) : null;
  if (agent) return <AgentDetail agent={agent} />;
  return connection ? <ConnectionDetail d={d} connection={connection} /> : null;
}

function TargetDetail({ d, target }: { d: Draft; target: WorkflowTarget | null }) {
  const stage = target && typeof target === 'object' && 'stage' in target ? d.workflow?.stages.find(s => s.id === target.stage) : null;
  if (stage) return <StageDetail d={d} stage={stage} />;
  if (target && d.workflow) return <><h3>Workflow input</h3><Markdown text={d.workflow.input || 'The project supplies its input when you run it.'} /></>;
  return <p className="muted">Select an agent or arrow on the board, or choose a component above, to inspect its instructions and configuration.</p>;
}

function SelectedDetail({ d, selection, target }: { d: Draft; selection: Selection | null; target: WorkflowTarget | null }) {
  return SelectionDetail({ d, selection }) ?? <TargetDetail d={d} target={target} />;
}

/** The actual board, with read-only selection and an accessible list of every component. */
export function BlockDetails({ block }: { block: GroupBlock }) {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [target, setTarget] = useState<WorkflowTarget | null>(null);
  const d = block.draft;
  const select = (s: Selection | null) => { setSelection(s); setTarget(null); };
  const pickStage = (id: string) => { setSelection(null); setTarget({ stage: id }); };
  return <div className="block-inspection">
    <div className="block-inspection-board sb-stage">
      <StructureCanvas groupOnly teamId={`inspect:${block.id}`} draft={d} issues={[]} selection={selection} onSelect={select} onInspect={select} onOpen={id => select({ kind: 'agent', id })} onOpenAgent={id => select({ kind: 'agent', id })} onWorkflow={t => { setTarget(t); setSelection(null); }} edit={() => null} locked canUndo={false} canRedo={false} onUndo={() => {}} onRedo={() => {}} />
    </div>
    <ComponentList d={d} selection={selection} target={target} select={select} pickStage={pickStage} />
    <section className="block-component-detail" aria-live="polite" aria-label="Selected component"><SelectedDetail d={d} selection={selection} target={target} /></section>
  </div>;
}

/** Lead with the author's overview; keep provenance and operation notes one click away. */
export function BlockDescription({text}:{text:string}) {
  const formatted=text.replace(/\bRig Control\b/g,'Sculptured').replace(/Architecture reference: (.+?), slide (\d+): (https:\/\/[^\s]+?)(?:\.)? Original Sculptured instructions, not course-provided code or an endorsement\. Reports-to\/watch arrows describe intent; agents use managed messages to coordinate\. Imported blocks require a folder and review; no agents start on import\./g,
    (_,title,slide,url)=>`**Architecture reference:** [${title} · slide ${slide}](${url.replace(/\.$/,'')})\n\nOriginal Sculptured instructions; the course did not supply this code or endorse it.\n\n- Reports-to and watch arrows describe intent. Agents coordinate through managed messages.\n- Choose a project folder and review instructions before starting.\n- Importing a block does not start agents.`);
  const paragraphs=formatted.split(/\n\s*\n/).filter(Boolean);
  return <div className="block-description"><Markdown text={paragraphs[0]??''}/>{paragraphs.length>1&&<details><summary>Background, source & usage notes</summary>{paragraphs.slice(1).map((p,i)=><Markdown key={i} text={p}/>)}</details>}</div>;
}
export function BlockFeatures({block}:{block:GroupBlock}) {
  const d=block.draft;
  return <ul className="block-features"><li>{d.agents.length} agent roles: {d.agents.slice(0,4).map(a=>a.name).join(', ')+(d.agents.length>4?` and ${d.agents.length-4} more`:'')||'ready to design'}</li><li>{d.workflow?`${d.workflow.stages.length} workflow stages with outcome routing`:`${d.connections.filter(c=>c.kind==='hands-work-to').length} handoffs · ${d.connections.filter(c=>c.kind!=='hands-work-to').length} coordination links`}</li><li>Editable copy · inherits your project folder</li></ul>;
}

function ReadableInstructions({text}:{text:string}) {
  if(text.length<500||/\n\s*\n|^[-#*]|```/m.test(text))return <Markdown text={text}/>;
  const sentences=text.split(/(?<=[.!?])\s+(?=[A-Z])/);
  if(sentences.length<3)return <Markdown text={text}/>;
  return <div className="block-instructions"><Markdown text={sentences[0]}/><ul>{sentences.slice(1).map((sentence,i)=><li key={i}><Markdown text={sentence}/></li>)}</ul></div>;
}
