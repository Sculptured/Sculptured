import { closedGroupBox, GroupVisibilityButton, useGroupVisibility } from './groupVisibility';
import { useEffect, useImperativeHandle, useMemo, useState, type Ref } from 'react';
import type { CompiledRig } from '../generated/CompiledRig';
import type { CompiledEdge } from '../generated/CompiledEdge';
import { type Layout } from '../layout';
import { HiddenAgents } from './HiddenAgents';
import { nameForId, WORDS, type NameMap } from '../vocab';
import type { AgentRow } from './model';
import { Icon } from './ui';
import { usePanZoom } from './viewport';
import { boxInView, useElementSize, visibleWorld } from './culling';
import type { TeamDraft } from '../generated/TeamDraft';
import { WorkflowGraph, type WorkflowTarget } from '../workflow/WorkflowGraph';
import { allArrowsWanted, QUIET_AFTER } from '../structure/arrowFocus';
import { logoRouteBox, useLogoMotion } from './boardStyle';
import { LogoBox } from './LogoNode';
import { ComponentChips } from '../structure/ComponentChips';
import { seatIdFor } from '../draft/ids';
import { AgentBox } from './AgentBox';
import { useGraphGeometry } from './graphGeometry';
import { pulseKey } from './graphPulses';

export type { Viewport } from './viewport';
export { forgetViewports } from './viewport';
export type GraphHandle = { fit: () => void; zoomBy: (factor: number) => void };

export { routeEdges, edgeGeometry } from './edgeGeometry';
export type { EdgeGeom } from './edgeGeometry';
export { edgeLabel } from './graphGeometry';
export { AgentBox, chipRow, CARD_CHIPS } from './AgentBox';
export { pulseKey, PULSE_MS, useHandoffPulses } from './graphPulses';

type Props = {
  observedLabels?: Record<string,string>;
  liveOnly?: boolean;
  draft?: TeamDraft;
  onWorkflow?: (target: WorkflowTarget) => void;
  rig: string;
  viewportKey?: string;
  compiled: CompiledRig;
  agents: Record<string, AgentRow>;
  /** Persisted positions; without them the layout is computed from the team. */
  layout?: Layout;
  names?: NameMap;
  selected?: string;
  /** Agents picked to message together. */
  picked?: Set<string>;
  onOpen: (id: string) => void;
  onAnswer: (id: string) => void;
  onSelect?: (id: string) => void;
  now: number;
  ref?: Ref<GraphHandle>;
  /** Edges (`pulseKey`) that just carried a handoff; they pulse briefly. */
  pulses?: Set<string>;
};

type Geometry = ReturnType<typeof useGraphGeometry>;
type Seen = ReturnType<typeof visibleWorld>;
type Reveal = ReturnType<typeof usePanZoom>['reveal'];
type EdgeEnds = { from: string; to: string };

/** What every layer of the graph shares: where things are, what is on screen and what is lit. */
type Scene = {
  rig: string; compiled: CompiledRig; g: Geometry; view: Seen; reveal: Reveal; collapsed: Set<string>;
  edgeInView: (e: EdgeEnds) => boolean; lit: (e: EdgeEnds) => string; liveOnly?: boolean;
};

function GroupBoxes({ s, setCollapsed }: { s: Scene; setCollapsed: (id: string, on: boolean) => void }) {
  const { compiled, g, view, reveal, collapsed } = s;
  return <>{compiled.pods.map((p) => {
    const original = g.layout.pods[p.id];
    if (!original) return null;
    const hidden = collapsed.has(p.id);
    const b = hidden ? closedGroupBox(original) : original;
    if (!boxInView(view, b)) return null;
    return (
      <div key={p.id} className={`group-box${hidden ? ' group-collapsed' : ''}`} style={{ left: b.x, top: b.y, width: b.w, height: b.h }}>
        <span className="group-title">
          <Icon name="group" size={12} />
          {p.label}
        </span>
        {hidden && <span className="group-hidden-count">{compiled.seats.filter(x => x.pod === p.id).length} agents · contents hidden</span>}
        <GroupVisibilityButton name={p.label || p.id} collapsed={hidden} onFocus={() => reveal({x:b.x+b.w-145,y:hidden ? b.y+b.h-36 : b.y-14,w:145,h:28})} onToggle={() => setCollapsed(p.id, !hidden)} />
      </div>
    );
  })}</>;
}

function ArrowMarkers({ rig }: { rig: string }) {
  return (
    <defs>
      <marker id={`arrow-${rig}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
        <path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head" />
      </marker>
      <marker id={`arrow-flow-${rig}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
        <path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head flowing" />
      </marker>
    </defs>
  );
}

function EdgePath({ e, d, flowing, pulsing, lit, rig }: { e: CompiledEdge; d: string; flowing: boolean; pulsing: boolean; lit: string; rig: string }) {
  return (
    <g>
      <path
        d={d}
        className={`edge ${e.enforced ? 'enforced' : 'intent'}${flowing ? ' flowing' : ''}${pulsing ? ' handoff-pulse' : ''}${lit}`}
        data-testid={`edge-${e.from}-${e.to}`}
        data-pulse={pulsing ? 'handoff' : undefined}
        markerEnd={`url(#${flowing ? 'arrow-flow' : 'arrow'}-${rig})`}
      />
      {/* Work moving: dashes travel along the line (still dashes when motion is reduced). */}
      {flowing && <path d={d} className="edge-flow" />}
    </g>
  );
}

function Edges({ s, agents, pulses, size }: { s: Scene; agents: Record<string, AgentRow>; pulses?: Set<string>; size: { w: number; h: number } }) {
  const { rig, g, edgeInView, lit, liveOnly } = s;
  return (
    <svg className="edges" width={size.w} height={size.h} aria-hidden="true">
      <ArrowMarkers rig={rig} />
      {g.drawnEdges.map((e) => {
        const geom = g.routes.get(e);
        if (!geom || !edgeInView(e)) return null;
        const flowing = !liveOnly && agents[e.from]?.state.key === 'working';
        const pulsing = !!(e.enforced && (e.kind === 'delegates_to' || liveOnly) && pulses?.has(pulseKey(e.from, e.to)));
        return <EdgePath key={`${e.kind}-${e.from}-${e.to}`} e={e} d={geom.d} flowing={flowing} pulsing={pulsing} lit={lit(e)} rig={rig} />;
      })}
    </svg>
  );
}

function EdgeLabels({ s }: { s: Scene }) {
  const { g, edgeInView, lit } = s;
  return <>{g.drawnEdges.map((e, i) => {
    const spot = g.spots.get(String(i));
    if (!spot || !edgeInView(e)) return null;
    return (
      <span
        key={`l-${e.kind}-${e.from}-${e.to}`}
        className={`edge-label ${e.enforced ? 'enforced' : 'intent'}${spot.wrap ? ' wrap' : ''}${lit(e)}`}
        data-testid={`edge-label-${e.from}-${e.to}`}
        style={{ left: spot.x, top: spot.y, maxWidth: spot.maxWidth }}
        aria-hidden="true"
      >
        {(g.routes.get(e)?.merged.length ?? 1) > 1 ? `${g.routes.get(e)!.merged.length} arrows` : g.label(e)}
      </span>
    );
  })}</>;
}

type CardEvents = { agents: Record<string, AgentRow>; selected?: string; picked?: Set<string>; now: number; onOpen: (id: string) => void; onAnswer: (id: string) => void; onSelect?: (id: string) => void; chipsFor: (seatId: string) => React.ReactNode; setHover: (f: (h: string | null) => string | null) => void; viewportEl: React.RefObject<HTMLElement | null> };

/** A card that is off screen keeps a bare focusable stub, so Tab still reaches it. */
function CardStub({ id, a, b, s, ev }: { id: string; a: AgentRow; b: Layout['seats'][string]; s: Scene; ev: CardEvents }) {
  const focusCard = () => {
    s.reveal(b);
    ev.onSelect?.(id);
    requestAnimationFrame(() => ev.viewportEl.current?.querySelector<HTMLElement>(`[data-agent="${CSS.escape(id)}"]`)?.focus({ preventScroll: true }));
  };
  return (
    <div className="card-slot card-stub" style={{ left: b.x, top: b.y, width: b.w, height: b.h }}>
      <button type="button" aria-label={`${a.name} — ${a.state.word}`} onFocus={focusCard} onClick={() => ev.onOpen(id)} />
    </div>
  );
}

function Card({ id, a, b, s, ev }: { id: string; a: AgentRow; b: Layout['seats'][string]; s: Scene; ev: CardEvents }) {
  const motion = useLogoMotion();
  const common = { a, selected: ev.selected === id, picked: !!ev.picked?.has(id), onOpen: () => ev.onOpen(id), onFocusBox: () => { s.reveal(b); ev.onSelect?.(id); } };
  return (
    <div className="card-slot" style={{ left: b.x, top: b.y, width: b.w, height: b.h }} onPointerEnter={() => ev.setHover(() => id)} onPointerLeave={() => ev.setHover((h) => (h === id ? null : h))}>
      {s.g.logos
        ? <LogoBox {...common} motion={motion} chips={ev.chipsFor(id)} />
        : <AgentBox {...common} onAnswer={() => ev.onAnswer(id)} now={ev.now} components={ev.chipsFor(id)} />}
    </div>
  );
}

function Cards({ s, ev }: { s: Scene; ev: CardEvents }) {
  const { compiled, g, view } = s;
  return <>{compiled.seats.map((seat) => {
    const b = g.layout.seats[seat.id];
    const a = ev.agents[seat.id];
    if (!b || !a || g.projection.hidden.has(seat.id) || g.fold.hidden.has(seat.id)) return null;
    return boxInView(view, b) ? <Card key={seat.id} id={seat.id} a={a} b={b} s={s} ev={ev} /> : <CardStub key={seat.id} id={seat.id} a={a} b={b} s={s} ev={ev} />;
  })}</>;
}

function FoldedAgents({ s, agents, names, onOpen }: { s: Scene; agents: Record<string, AgentRow>; names: NameMap; onOpen: (id: string) => void }) {
  const { g } = s;
  return <>{([['more', g.fold.more], ['deeper', g.fold.deeper]] as const).flatMap(([kind, groups]) => [...groups].map(([at, ids]) => {
    const box = g.layout.seats[ids[0]];
    if (!box || g.projection.hidden.has(at)) return null;
    const name = agents[at]?.name ?? nameForId(at, names);
    return <HiddenAgents key={`${kind}-${at}`} box={box} ids={ids} kind={kind} relation={kind === 'more' ? `spawned by ${name}` : `below ${name}`} agents={agents} onOpen={onOpen} />;
  }))}</>;
}

/** Slots drawn on the canvas for agents the team has not started yet. */
function PlannedSlots({ s, names }: { s: Scene; names: NameMap }) {
  const { compiled, g, liveOnly } = s;
  const inTeam = new Set(compiled.seats.map((x) => x.id));
  const notStarted = liveOnly ? [] : Object.keys(g.layout.seats).filter((id) => !inTeam.has(id));
  return <>{notStarted.filter(id => !g.projection.hidden.has(id)).map((id) => {
    const b = g.layout.seats[id];
    return (
      <div key={id} className="card-slot" style={{ left: b.x, top: b.y, width: b.w, height: b.h }}>
        <div className="agent-box st-planned placeholder">
          <span className="box-main">{WORDS.notStartedAgent(nameForId(id, names))}</span>
        </div>
      </div>
    );
  })}</>;
}

function HandoffAnnouncement({ compiled, agents, names, pulses }: { compiled: CompiledRig; agents: Record<string, AgentRow>; names: NameMap; pulses?: Set<string> }) {
  const said = compiled.edges
    .filter((e) => e.kind === 'delegates_to' && pulses?.has(pulseKey(e.from, e.to)))
    .map((e) => `${agents[e.from]?.name ?? nameForId(e.from, names)} handed work to ${agents[e.to]?.name ?? nameForId(e.to, names)}.`)
    .join(' ');
  return <p className="sr-only" aria-live="polite" data-testid="handoff-announce">{said}</p>;
}

function OffCanvas({ seats, agents, names, onOpen }: { seats: CompiledRig['seats']; agents: Record<string, AgentRow>; names: NameMap; onOpen: (id: string) => void }) {
  if (seats.length === 0) return null;
  return (
    <section className="off-canvas" aria-label="Not on the canvas">
      <h2>Not on the canvas</h2>
      <p>These agents ran in this team's last start but are no longer part of it. Their history is kept.</p>
      <ul>
        {seats.map((s) => {
          const a = agents[s.id];
          return (
            <li key={s.id}>
              <button type="button" className="btn" onClick={() => onOpen(s.id)} aria-label={a?.seat ? `${a.name} — ${a.state.word}` : WORDS.unknownAgent(nameForId(s.id, names))}>
                {a?.name ?? nameForId(s.id, names)}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** Only what is on screen (and a margin) is drawn in full: a big run's off-screen cards, groups and
 * arrows cost nothing. The rounded view changes only now and then. */
function useVisibleWorld(viewportEl: React.RefObject<HTMLElement | null>, vp: Parameters<typeof visibleWorld>[0]) {
  const size = useElementSize(viewportEl);
  const seen = visibleWorld(vp, size.w, size.h);
  const viewKey = seen ? `${seen.x0},${seen.y0},${seen.x1},${seen.y1}` : '';
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the rounded view changes only now and then
  return useMemo(() => seen, [viewKey]);
}

/** Returning from an agent pane: focus the box that was selected. */
function useRefocusSelected(viewportEl: React.RefObject<HTMLElement | null>, selected?: string) {
  useEffect(() => {
    if (!selected) return;
    const a = document.activeElement;
    if (a && a !== document.body) return;
    viewportEl.current?.querySelector<HTMLElement>(`[data-agent="${CSS.escape(selected)}"]`)?.focus({ preventScroll: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

/** Components show as chips on the agent they belong to, read from the draft behind each running seat. */
function useChipsFor(draft?: TeamDraft) {
  const draftAgentBySeat = useMemo(() => new Map(draft ? draft.agents.map(a => [seatIdFor(draft, a.id) ?? '', a] as const) : []), [draft]);
  return (seatId: string) => { const da = draftAgentBySeat.get(seatId); return da ? <ComponentChips agent={da} draft={draft} /> : undefined; };
}

const pinScroll = (e: React.UIEvent<HTMLElement>) => { e.currentTarget.scrollTop = 0; e.currentTarget.scrollLeft = 0; };

function WorkflowLayer({ s, draft, onWorkflow }: { s: Scene; draft?: TeamDraft; onWorkflow?: (target: WorkflowTarget) => void }) {
  if (!draft || !onWorkflow) return null;
  return <WorkflowGraph collapsed={s.collapsed} draft={draft} onOpen={onWorkflow} onReveal={s.reveal} readOnly agentBox={s.g.logos ? logoRouteBox : undefined} />;
}

/** A press on empty canvas starts a pan; one on a card or link does not. */
const canPan = (e: React.PointerEvent) => e.button === 0 && !(e.target as HTMLElement).closest('button, a');

export function LiveGraph({ viewportKey, rig, compiled, agents, layout: given, names = {}, selected, picked, onOpen, onAnswer, onSelect, now, ref, pulses, draft, onWorkflow, observedLabels, liveOnly }: Props) {
  const [hover, setHover] = useState<string | null>(null);
  const { collapsed, setCollapsed } = useGroupVisibility(rig);
  const g = useGraphGeometry({ compiled, given, liveOnly, draft, collapsed, observedLabels });
  const { layout, workflowDraft } = g;
  const chipsFor = useChipsFor(draft);
  const { el: viewportEl, vp, fit, zoomAt, startPan, reveal } = usePanZoom(viewportKey ?? rig, g.bounds, !liveOnly && compiled.edges.some(e => e.kind === "spawned_by"));
  useImperativeHandle(ref, () => ({ fit, zoomBy: (f: number) => zoomAt(f) }), [fit, zoomAt]);
  const view = useVisibleWorld(viewportEl, vp);
  useRefocusSelected(viewportEl, selected);
  const onPointerDown = (e: React.PointerEvent) => { if (canPan(e)) startPan(e); };
  // Busy runs draw arrows faintly; the pointed-at or selected agent's arrows come forward.
  const quiet = !allArrowsWanted() && g.drawnEdges.length > QUIET_AFTER;
  const focused = hover ?? selected ?? null;
  const s: Scene = {
    rig, compiled, g, view, reveal, collapsed, liveOnly,
    edgeInView: (e) => edgeInView(e, g, view),
    lit: (e) => (quiet && focused && (e.from === focused || e.to === focused) ? ' is-focus' : ''),
  };
  const ev: CardEvents = { agents, selected, picked, now, onOpen, onAnswer, onSelect, chipsFor, setHover, viewportEl };
  return (
    <div className={`graph-wrap${quiet ? ' arrows-quiet' : ''}${g.logos ? ' board-logos' : ''}`}>
      <div className="graph-viewport" ref={viewportEl} onPointerDown={onPointerDown} onScroll={workflowDraft ? pinScroll : undefined} data-testid="graph-viewport">
        <div className="graph-world" data-testid="graph-world" style={{ transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.k})`, width: layout.width, height: layout.height }}>
          <GroupBoxes s={s} setCollapsed={setCollapsed} />
          <Edges s={s} agents={agents} pulses={pulses} size={{ w: layout.width + 400, h: layout.height + 200 }} />
          <WorkflowLayer s={s} draft={workflowDraft} onWorkflow={onWorkflow} />
          <EdgeLabels s={s} />
          <Cards s={s} ev={ev} />
          <FoldedAgents s={s} agents={agents} names={names} onOpen={onOpen} />
          <PlannedSlots s={s} names={names} />
        </div>
      </div>
      <HandoffAnnouncement compiled={compiled} agents={agents} names={names} pulses={pulses} />
      <OffCanvas seats={compiled.seats.filter((x) => !layout.seats[x.id])} agents={agents} names={names} onOpen={onOpen} />
    </div>
  );
}

/** Whether an arrow's two ends (and the span between them) touch the visible world. */
function edgeInView(e: EdgeEnds, g: Geometry, view: Seen) {
  const a = g.projection.boxes[e.from] ?? g.layout.seats[e.from], b = g.projection.boxes[e.to] ?? g.layout.seats[e.to];
  if (!a || !b) return true;
  const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
  return boxInView(view, { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y });
}
