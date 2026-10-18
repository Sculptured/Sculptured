import { useId, useState, type PointerEvent, type ButtonHTMLAttributes } from 'react';
import { ArrowHandles } from '../structure/ArrowHandles';
import type { TeamDraft } from '../generated/TeamDraft';
import type { CompiledEdge } from '../generated/CompiledEdge';
import { CARD_H, CARD_W, type Box } from '../layout';
import { projectGroups, routeGroupEdges } from '../shell/groupVisibility';
import { groupOf } from '../draft/ids';
import { drawnGroups } from '../draft/geometry';
import { placeLabels } from '../shell/labels';
import type { WorkflowStage } from '../generated/WorkflowStage';
import { stageOrder } from './itemBoardModel';
import { InputCardFace } from '../structure/InputMaterial';
import './workflow.css';

export type WorkflowTarget = 'input' | 'settings' | 'run' | { stage: string };
export function inputBox(draft: TeamDraft): Box {
  if (draft.workflow?.input_position) return { ...draft.workflow.input_position, w:CARD_W, h:CARD_H };
  return { x: Math.min(40, ...draft.agents.map(a => a.x)) - CARD_W - 120, y: Math.min(40, ...draft.agents.map(a => a.y)), w: CARD_W, h: CARD_H };
}

/** Where a stage is drawn: its agent's card, or its own box for a check, person, merge or review-panel stage. */
export function stageNode(s: WorkflowStage): string {
  return (s.kind ?? 'agent') === 'agent' && !s.reviewers?.length ? s.agent : `$stage:${s.id}`;
}

/** Boxes for stages without a single agent: where they were dragged to, else a column right of the agents, in route order. */
export function stageBoxes(draft: TeamDraft): Record<string, Box> {
  const w = draft.workflow;
  if (!w) return {};
  const boxed = stageOrder(w).map(id => w.stages.find(s => s.id === id)!).filter(s => stageNode(s) !== s.agent);
  const x = Math.max(40, ...draft.agents.map(a => a.x)) + CARD_W + 120, y = Math.min(40, ...draft.agents.map(a => a.y));
  return Object.fromEntries(boxed.map((s, i) => [stageNode(s), { ...(s.position ?? { x, y: y + i * (CARD_H + 40) }), w: CARD_W, h: CARD_H }]));
}

/** Display-only routes: never write dynamic branches into the ordinary handoff graph. */
export function workflowLinks(draft: TeamDraft) {
  const w = draft.workflow;
  if (!w) return [];
  const nodeOf = (id: string) => { const s = w.stages.find(s => s.id === id); return s ? stageNode(s) : ''; };
  const links: { id: string; from: string; to: string; label: string; target: WorkflowTarget; pool?: boolean }[] = [
    { id: 'input', from: '$workflow-input', to: w.dispatcher, label: 'Input', target: 'input' },
    { id: 'dispatch', from: w.dispatcher, to: nodeOf(w.entry), label: `Dispatch N · max ${w.max_tasks}`, target: 'settings' },
  ];
  for (const stage of w.stages) for (const worker of stage.workers ?? []) links.push({id:`${stage.id}:worker:${worker}`,from:stage.agent,to:worker,label:'Parallel worker pool',target:{stage:stage.id},pool:true});
  for (const stage of w.stages) for (const reviewer of stage.reviewers ?? []) links.push({id:`${stage.id}:reviewer:${reviewer}`,from:stageNode(stage),to:reviewer,label:'Reviewer',target:{stage:stage.id},pool:true});
  for (const stage of w.stages) if (stage.on_limit && stage.max_rounds) links.push({ id: `${stage.id}:on_limit`, from: stageNode(stage), to: nodeOf(stage.on_limit), label: `after ${stage.max_rounds} rounds`, target: { stage: stage.id }, pool: true });
  for (const stage of w.stages) for (const [outcome, to] of Object.entries(stage.routes)) {
    links.push({ id: `${stage.id}:${outcome}`, from: stageNode(stage), to: to === null ? w.collector : nodeOf(to),
      label: to === null ? `${outcome} · all N accepted` : outcome, target: { stage: stage.id } });
  }
  return links;
}

const KIND_WORD: Record<string, string> = { check: 'Check', human: 'Person decides', merge: 'Merge', panel: 'Review panel' };

function StageBox({ draft, stage, box, onOpen, onReveal, controls }: { draft: TeamDraft; stage: WorkflowStage; box: Box; onOpen: (t: WorkflowTarget) => void; onReveal: (b: Box) => void; controls?: ButtonHTMLAttributes<HTMLButtonElement> }) {
  const kind = stage.reviewers?.length ? 'panel' : stage.kind ?? 'agent';
  const names = (stage.reviewers ?? []).map(id => draft.agents.find(a => a.id === id)?.name ?? id).join(', ');
  const detail = kind === 'check' ? `$ ${stage.command ?? ''}` : kind === 'human' ? (stage.prompt ?? '').slice(0, 80)
    : kind === 'merge' ? `into ${stage.into || 'the current branch'}` : `${names} · ${stage.join === 'any' ? 'first approval' : 'all approve'}`;
  return <button {...controls} className={`workflow-stage-node kind-${kind}${controls ? ' movable' : ''}`} style={{ left: box.x, top: box.y, width: box.w, height: box.h }}
    aria-label={`${KIND_WORD[kind]}: ${stage.name}`} onFocus={controls?.onFocus ?? (() => onReveal(box))} onClick={controls?.onClick ?? (() => onOpen({ stage: stage.id }))}>
    <strong>{stage.name}</strong><span>{KIND_WORD[kind]}</span><p>{detail}</p>
  </button>;
}

/** `agentBox`: where an agent's arrows end inside its card's slot (Logos style), else the slot itself. */
function geometry(draft: TeamDraft, collapsed: ReadonlySet<string> = new Set(), cardHeight: (id: string) => number = () => CARD_H, agentBox: (b: Box) => Box = (b) => b) {
  const input = inputBox(draft), links = workflowLinks(draft);
  const boxes = Object.fromEntries([...draft.agents.map(a => [a.id, agentBox({ x: a.x, y: a.y, w: CARD_W, h: cardHeight(a.id) })] as const), ['$workflow-input', input], ...Object.entries(stageBoxes(draft))]);
  const edges: CompiledEdge[] = links.map(l => ({ from: l.from, to: l.to, kind: 'delegates_to', enforced: !l.pool }));
  const projection = projectGroups(boxes, Object.fromEntries(draft.agents.map(a => [a.id, groupOf(a)])), Object.fromEntries(drawnGroups(draft).map(g => [g.id, g])), collapsed);
  const routes = routeGroupEdges(edges, projection);
  const labels = placeLabels(links.flatMap((l, i) => { const geom = routes.get(edges[i]); return geom ? [{ key: l.id, geom, text: l.label }] : []; }), Object.values(projection.boxes));
  return { input, links, edges, routes, labels };
}

/** Include curved feedback routes and labels when fitting the board. */
export function workflowBounds(draft: TeamDraft): Box[] {
  const g = geometry(draft);
  return [...(draft.workflow?.dispatcher ? [g.input] : []), ...Object.values(stageBoxes(draft)), ...[...g.routes.values()].flatMap(r => r.pts.map(p => ({ ...p, w:1, h:1 }))),
    ...[...g.labels.values()].map(p => ({ x:p.x-p.w/2, y:p.y-p.h/2, w:p.w, h:p.h }))];
}

export function WorkflowGraph({ draft, onOpen, onReveal, onReconnect, inputControls, stageControls, readOnly = false, collapsed, cardHeight, agentBox }: { cardHeight?: (id: string) => number; agentBox?: (b: Box) => Box; inputControls?: ButtonHTMLAttributes<HTMLButtonElement>; stageControls?: (stage: string) => ButtonHTMLAttributes<HTMLButtonElement>; onReconnect?: (id: string, event: PointerEvent<HTMLButtonElement>) => void; collapsed?: ReadonlySet<string>; draft: TeamDraft; onOpen: (target: WorkflowTarget) => void; onReveal: (b: Box) => void; readOnly?: boolean }) {
  const id = useId(), { input, links, edges, routes, labels } = geometry(draft, collapsed, cardHeight, agentBox);
  const [selected, select] = useState<string | null>(null);
  const selectedIndex = links.findIndex(l => l.id === selected), selectedLink = links[selectedIndex];
  const selectedGeometry = routes.get(edges[selectedIndex]);
  return <>
    <svg className="edges" aria-hidden="true"><defs><marker id={id} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head" /></marker></defs>
      {edges.map((e, i) => routes.has(e) && <g key={links[i].id}>
        <path className="edge-hit" data-workflow-route={links[i].id} d={routes.get(e)?.d} onPointerDown={ev => ev.stopPropagation()} onClick={() => { select(links[i].id); if (!onReconnect) onOpen(links[i].target); }} />
        <path className={`edge ${e.enforced ? 'enforced' : 'intent'}${selected === links[i].id ? ' selected' : ''}`} d={routes.get(e)?.d} markerEnd={e.enforced ? `url(#${id})` : undefined} />
      </g>)}

    </svg>
    {onReconnect && selectedLink && selectedGeometry && !selectedLink.pool && selectedLink.id !== 'input' && <ArrowHandles geometry={selectedGeometry} label={selectedLink.label} ends={['to']} onDrag={(_end, event) => onReconnect(selectedLink.id, event)} onEdit={() => onOpen(selectedLink.target)} />}
    {links.map(l => { const p = labels.get(l.id); return p && <button key={l.id} className={`edge-label sb-edge-label ${l.pool ? 'intent' : 'enforced'}${p.wrap ? ' wrap' : ''}`} style={{ left: p.x, top: p.y, maxWidth: p.maxWidth }} aria-label={`${l.pool ? 'Worker capacity' : 'Workflow route'}: ${l.label}`} onFocus={() => onReveal({ x:p.x-80,y:p.y-20,w:160,h:40 })} onClick={() => { select(l.id); onOpen(l.target); }}>{l.label}</button>; })}
    {Object.entries(stageBoxes(draft)).map(([node, box]) => { const s = draft.workflow!.stages.find(x => stageNode(x) === node)!; return <StageBox key={node} draft={draft} stage={s} box={box} onOpen={onOpen} onReveal={onReveal} controls={stageControls?.(s.id)} />; })}
    {draft.workflow!.dispatcher && <button {...inputControls} className={`workflow-input-node input-card${draft.workflow!.input.trim() ? '' : ' is-empty'}${inputControls ? ' movable' : ''}`} style={{ left: input.x, top: input.y, width: input.w, height: input.h }} onFocus={inputControls?.onFocus ?? (() => onReveal(input))} onClick={inputControls?.onClick ?? (() => onOpen('input'))} aria-label="Input: links, notes and ideas">
      <InputCardFace text={draft.workflow!.input} room={2} action={readOnly ? 'Open to read' : draft.workflow!.input.trim() ? 'Open to edit' : 'Open to add'} />
    </button>}
  </>;
}

export function WorkflowAgentActions({ draft, agent, onOpen }: { draft: TeamDraft; agent: string; onOpen: (target: WorkflowTarget) => void }) {
  const w = draft.workflow!;
  return <span className="workflow-agent-actions">
    {agent === w.dispatcher && <button className="link-btn" onClick={() => onOpen('settings')}>Dispatch N · settings</button>}
    {w.stages.filter(s => s.agent === agent || s.workers?.includes(agent)).map(s => <button key={s.id} className="link-btn" onClick={() => onOpen({ stage: s.id })}>{s.name} · {1+(s.workers?.length ?? 0)} workers · branches</button>)}
    {agent === w.collector && <button className="link-btn" onClick={() => onOpen('run')}>All N accepted · results</button>}
  </span>;
}
