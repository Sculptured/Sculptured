import { closedGroupBox, GroupVisibilityButton, projectGroups, routeGroupEdges, useGroupVisibility } from './groupVisibility';
import { needsAttention } from '../status';
import { useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode, type Ref } from 'react';
import type { CompiledRig } from '../generated/CompiledRig';
import type { CompiledEdge } from '../generated/CompiledEdge';
import type { EventRecord } from '../generated/EventRecord';
import { chipText, shortAge, tokensText, type ToolChip } from '../herdr';
import { computeLayout, withLiveChildren, type Layout } from '../layout';
import { foldChildren } from './childFold';
import { HiddenAgents } from './HiddenAgents';
import { EDGE_WORDS, nameForId, NOT_ENFORCED, WORDS, type NameMap } from '../vocab';
import type { AgentRow } from './model';
import { Btn, Icon, StateMark, StuckMark, VendorMark, WaitingMark } from './ui';
import { usePanZoom } from './viewport';
import { boxInView, useElementSize, visibleWorld } from './culling';
import { placeLabels } from './labels';
import type { TeamDraft } from '../generated/TeamDraft';
import { seatIdFor } from '../draft/ids';
import { WorkflowGraph, workflowBounds, type WorkflowTarget } from '../workflow/WorkflowGraph';

export type { Viewport } from './viewport';
export { forgetViewports } from './viewport';
export type GraphHandle = { fit: () => void; zoomBy: (factor: number) => void };

import { routeEdges } from './edgeGeometry';
import { useSecondClock } from './secondClock';
import { allArrowsWanted, QUIET_AFTER } from '../structure/arrowFocus';
import { CheckMark } from './CheckMark';
import { logoRouteBox, logoRouteBoxes, useBoardStyle, useLogoMotion } from './boardStyle';
import { LogoBox } from './LogoNode';
import { ComponentChips } from '../structure/ComponentChips';
export { routeEdges, edgeGeometry } from './edgeGeometry';
export type { EdgeGeom } from './edgeGeometry';

export function edgeLabel(e: CompiledEdge): string {
  return `${EDGE_WORDS[e.kind]}${e.enforced ? '' : ` · ${NOT_ENFORCED}`}`;
}

/** Key of the edge a handoff travels along. */
export const pulseKey = (from: string, to: string) => `${from}\u2192${to}`;

/** How long an edge pulses after a handoff. */
export const PULSE_MS = 1800;

/**
 * Edges that carried a handoff in the last `PULSE_MS`: each live `handed-over` event (on the
 * receiving seat) pulses the edge from every upstream seat it names. Events already in the
 * feed when the graph opens do not pulse.
 */
export function useHandoffPulses(feed: Record<string, EventRecord[]>, ms = PULSE_MS): Set<string> {
  const seen = useRef<Set<number> | null>(null);
  // Each pulse ends on its own timer. The timers live here, not in the effect's cleanup:
  // the feed changes on every event, and a cleanup would cancel a pulse's end for good.
  const timers = useRef<Set<ReturnType<typeof setTimeout>>>(new Set());
  const [active, setActive] = useState<Map<string, number>>(() => new Map());
  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const t of pending) clearTimeout(t);
      pending.clear();
    };
  }, []);
  const handed = useMemo(
    () =>
      Object.values(feed).flatMap((list) =>
        list.flatMap((e) =>
          e.body.type === 'handed-over' && e.subject.kind === 'seat' ? [{ seq: e.seq, to: e.subject.id, from: e.body.from.map((f) => f.seat) }] : [],
        ),
      ),
    [feed],
  );
  useEffect(() => {
    if (seen.current === null) {
      seen.current = new Set(handed.map((h) => h.seq));
      return;
    }
    const fresh = handed.filter((h) => !seen.current!.has(h.seq));
    if (fresh.length === 0) return;
    for (const h of fresh) seen.current.add(h.seq);
    const keys = fresh.flatMap((h) => h.from.map((f) => pulseKey(f, h.to)));
    setActive((m) => {
      const next = new Map(m);
      for (const k of keys) next.set(k, (next.get(k) ?? 0) + 1);
      return next;
    });
    const t = setTimeout(() => {
      timers.current.delete(t);
      setActive((m) => {
        const next = new Map(m);
        for (const k of keys) {
          const n = (next.get(k) ?? 1) - 1;
          if (n <= 0) next.delete(k);
          else next.set(k, n);
        }
        return next;
      });
    }, ms);
    timers.current.add(t);
  }, [handed, ms]);
  return useMemo(() => new Set(active.keys()), [active]);
}

function Chip({ chip }: { chip: ToolChip }) {
  const now = useSecondClock(chip.status === 'running') || Date.now();
  return <span className={`chip chip-${chip.status}`}>{chipText(chip, now)}</span>;
}

/** Chips a card shows in its one chip row; the rest are counted as "+N". */
export const CARD_CHIPS = 2;

/** The card's chip row: the first CARD_CHIPS tools, and how many more the turn used. */
export function chipRow(chips: ToolChip[], toolCount: number, room = CARD_CHIPS): { shown: ToolChip[]; more: number } {
  const shown = chips.slice(0, room);
  return { shown, more: Math.max(0, toolCount - shown.length) };
}

function ChipRow({ chips, toolCount }: { chips: ToolChip[]; toolCount: number }) {
  const { shown, more } = chipRow(chips, toolCount);
  return (
    <>
      {shown.map((c) => (
        <Chip key={c.name} chip={c} />
      ))}
      {more > 0 && (
        <span className="chip chip-more" title={`${more} more tools this turn`}>
          +{more}
        </span>
      )}
    </>
  );
}

export function AgentBox({
  a,
  selected,
  picked = false,
  onOpen,
  onAnswer,
  onFocusBox,
  now,
  components,
}: {
  a: AgentRow;
  selected: boolean;
  /** One of several agents picked to message together. */
  picked?: boolean;
  onOpen: () => void;
  onAnswer: () => void;
  onFocusBox?: () => void;
  now: number;
  /** The components this agent uses, as chips. */
  components?: ReactNode;
}) {
  const seat = a.seat;
  const reason = seat && needsAttention(seat.lifecycle) ? seat.lifecycle_reason : null;
  const chips = seat?.lifecycle === 'running' || seat?.lifecycle === 'stale' ? (a.activity?.chips ?? []) : [];
  const last = a.activity?.lastText?.split('\n').find((l) => l.trim()) ?? null;
  const waiting = a.waitingFor ?? [];
  const stuck = a.state.key === 'stuck' ? a.stuck : undefined;
  const label = seat ? `${a.name} — ${a.state.word}${stuck || waiting.length ? `, ${a.summary}` : ''}` : WORDS.unknownAgent(a.name);
  return (
    <div className={`agent-box ${a.vendor.cls} st-${a.state.key} tone-box-${a.state.tone}${selected ? ' selected' : ''}${picked ? ' picked' : ''}`}>
      <button
        type="button"
        className="box-main"
        aria-label={label}
        aria-pressed={selected}
        data-agent={a.id}
        onClick={onOpen}
        onFocus={onFocusBox}
      >
        <span className="box-head">
          <VendorMark v={a.vendor} size="md" />
          <span className="box-name">{a.name}</span>
        </span>
        <span className="box-state">
          <StateMark state={a.state} pill />
          {a.state.key === 'working' && seat?.current_tool && <span className="box-tool"> · {seat.current_tool.toLowerCase()}</span>}
          {seat?.check && <CheckMark check={seat.check} />}
        </span>
        {components}
        {/* The tool line shows only when there is something to say: stuck, waiting, or a tool. */}
        {(stuck || waiting.length > 0 || chips.length > 0) && (
          <span className="box-chips">
            {stuck ? (
              <StuckMark reason={stuck.reason} className="box-waiting" />
            ) : waiting.length > 0 ? (
              // Waiting for a handoff: no turn yet, so this line says what it waits for.
              <WaitingMark names={waiting} className="box-waiting" />
            ) : (
              <ChipRow chips={chips} toolCount={a.activity?.toolCount ?? chips.length} />
            )}
          </span>
        )}
        {reason ? (
          <span className="box-last reason clamp" data-raw-evidence data-raw-evidence-title="" title={reason}>
            {reason}
          </span>
        ) : a.activity?.limitNote ? (
          <span className="box-last tone-amber" data-raw-evidence>
            {a.activity.limitNote}
          </span>
        ) : (
          <span className="box-last" data-raw-evidence>
            {last ? `› ${last}` : '› no reply yet'}
          </span>
        )}
      </button>
      <div className="box-foot">
        <span className="box-foot-text">
          {a.state.key === 'needs-you' ? '' : `${a.vendor.name} · `}
          {tokensText(seat)}
        </span>
        <span className="box-age" title="Last activity">{shortAge(a.lastMs, now)}</span>
        {a.state.key === 'needs-you' && (
          <Btn variant="danger" size="sm" icon="message" className="box-answer" onClick={onAnswer} aria-label={`Answer ${a.name}`} tip="Open the agent and answer it">
            Answer
          </Btn>
        )}
      </div>
    </div>
  );
}

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

export function LiveGraph({ viewportKey, rig, compiled, agents, layout: given, names = {}, selected, picked, onOpen, onAnswer, onSelect, now, ref, pulses, draft, onWorkflow, observedLabels, liveOnly }: Props) {
  const [hover, setHover] = useState<string | null>(null);
  const { collapsed, setCollapsed } = useGroupVisibility(rig);
  const label = (e:CompiledEdge) => observedLabels?.[`${e.from}→${e.to}`] ?? edgeLabel(e);
  const layout = useMemo(() => given ? (liveOnly ? given : withLiveChildren(given, compiled)) : computeLayout(compiled), [given, compiled, liveOnly]);
  const workflowDraft = useMemo(() => draft?.workflow ? { ...draft, agents: draft.agents.map(a => {
    const b = layout.seats[seatIdFor(draft, a.id) ?? '']; return b ? { ...a, x:b.x, y:b.y } : a;
  }), groups: draft.groups.map(g => ({ ...g, ...layout.pods[g.id] })) } : undefined, [draft, layout]);
  // Components show as chips on the agent they belong to, read from the draft behind each running seat.
  const draftAgentBySeat = useMemo(() => new Map(draft ? draft.agents.map(a => [seatIdFor(draft, a.id) ?? '', a] as const) : []), [draft]);
  const chipsFor = (seatId: string) => { const da = draftAgentBySeat.get(seatId); return da ? <ComponentChips agent={da} draft={draft} /> : undefined; };
  const projection = useMemo(() => projectGroups(layout.seats, Object.fromEntries([...(draft?.agents.map(a => [seatIdFor(draft, a.id)!, a.group_id ?? 'team']) ?? []), ...compiled.seats.map(s => [s.id, s.pod])]), layout.pods, collapsed), [layout, compiled.seats, collapsed, draft]);
  // Many or deep spawned agents fold into "+N more" / "+N deeper"; their saved boxes stay put.
  const fold = useMemo(() => foldChildren(compiled), [compiled]);
  const drawnEdges = useMemo(() => compiled.edges.filter((e) => !fold.hidden.has(e.from) && !fold.hidden.has(e.to)), [compiled.edges, fold]);
  // Logos style: same slots, but the arrows end at the logo, not at the slot's edge.
  const logos = useBoardStyle() === 'logos';
  const motion = useLogoMotion();
  const routes = useMemo(() => routeGroupEdges(drawnEdges, logos ? { ...projection, boxes: logoRouteBoxes(projection.boxes) } : projection), [drawnEdges, projection, logos]);
  // Labels sit beside their curves, never on a curve, a card or another label.
  const spots = useMemo(
    () =>
      placeLabels(
        drawnEdges.flatMap((e, i) => {
          const g = routes.get(e);
          const text = g && (g.merged.length > 1 ? `${g.merged.length} arrows` : label(e));
          return g && text ? [{ key: String(i), geom: g, text }] : [];
        }),
        Object.values(projection.boxes),
      ),
    [drawnEdges, routes, projection],
  );
  // Everything drawn, including connection arcs above or below the boxes.
  const bounds = useMemo(() => {
    // Visibility is a lens over the saved drawing, so toggles never trigger auto-fit.
    const fitRoutes = routeEdges(compiled.edges, layout.seats);
    const fitLabels = placeLabels(compiled.edges.flatMap((e, i) => {
      const geom = fitRoutes.get(e); return geom && label(e) ? [{ key: String(i), geom, text: label(e) }] : [];
    }), Object.values(layout.seats));
    const mids = [...fitRoutes.values()].map((g) => g.mid.y);
    const controls = [...fitRoutes.values()].flatMap(g=>g.pts);
    const tags = [...fitLabels.values()];
    const boxes = [...Object.values(layout.seats), ...Object.values(layout.pods), ...(workflowDraft ? workflowBounds(workflowDraft) : [])];
    return {
      x0: Math.min(0, ...boxes.map((b) => b.x), ...controls.map(p=>p.x-12)),
      y0: Math.min(0, ...boxes.map((b) => b.y), ...mids.map((y) => y - 16), ...tags.map((t) => t.y - t.h / 2 - 4)),
      x1: Math.max(workflowDraft ? 0 : layout.width, ...boxes.map((b) => b.x + b.w), ...controls.map(p=>p.x+12)),
      y1: Math.max(workflowDraft ? 0 : layout.height, ...boxes.map((b) => b.y + b.h), ...mids.map((y) => y + 16), ...tags.map((t) => t.y + t.h / 2 + 4)),
    };
  }, [compiled.edges, layout, workflowDraft]);
  const { el: viewportEl, vp, fit, zoomAt, startPan, reveal } = usePanZoom(viewportKey ?? rig, bounds, !liveOnly && compiled.edges.some(e => e.kind === "spawned_by"));

  useImperativeHandle(ref, () => ({ fit, zoomBy: (f: number) => zoomAt(f) }), [fit, zoomAt]);

  // Only what is on screen (and a margin) is drawn in full: a big run's off-screen cards, groups and
  // arrows cost nothing. Off-screen agents keep a bare focusable stub, so Tab still reaches them.
  const size = useElementSize(viewportEl);
  const seen = visibleWorld(vp, size.w, size.h);
  const viewKey = seen ? `${seen.x0},${seen.y0},${seen.x1},${seen.y1}` : '';
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the rounded view changes only now and then
  const view = useMemo(() => seen, [viewKey]);
  const edgeInView = (e: { from: string; to: string }) => {
    const a = projection.boxes[e.from] ?? layout.seats[e.from], b = projection.boxes[e.to] ?? layout.seats[e.to];
    if (!a || !b) return true;
    const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
    return boxInView(view, { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y });
  };

  // Returning from an agent pane: focus the box that was selected.
  useEffect(() => {
    if (!selected) return;
    const a = document.activeElement;
    if (a && a !== document.body) return;
    viewportEl.current?.querySelector<HTMLElement>(`[data-agent="${CSS.escape(selected)}"]`)?.focus({ preventScroll: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest('button, a')) return;
    startPan(e);
  };

  const inTeam = new Set(compiled.seats.map((s) => s.id));
  const offCanvas = compiled.seats.filter((s) => !layout.seats[s.id]);
  const notStarted = liveOnly ? [] : Object.keys(layout.seats).filter((id) => !inTeam.has(id));
  // Busy runs draw arrows faintly; the pointed-at or selected agent's arrows come forward.
  const quiet = !allArrowsWanted() && drawnEdges.length > QUIET_AFTER;
  const focused = hover ?? selected ?? null;
  const lit = (e: { from: string; to: string }) => (quiet && focused && (e.from === focused || e.to === focused) ? ' is-focus' : '');
  const W = layout.width + 400;
  const H = layout.height + 200;

  return (
    <div className={`graph-wrap${quiet ? ' arrows-quiet' : ''}${logos ? ' board-logos' : ''}`}>
      <div
        className="graph-viewport"
        ref={viewportEl}
        onPointerDown={onPointerDown}
        onScroll={workflowDraft ? e => { e.currentTarget.scrollTop = 0; e.currentTarget.scrollLeft = 0; } : undefined}
        data-testid="graph-viewport"
      >
        <div
          className="graph-world"
          data-testid="graph-world"
          style={{ transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.k})`, width: layout.width, height: layout.height }}
        >
          {compiled.pods.map((p) => {
            const original = layout.pods[p.id];
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
                {hidden && <span className="group-hidden-count">{compiled.seats.filter(s => s.pod === p.id).length} agents · contents hidden</span>}
                <GroupVisibilityButton name={p.label || p.id} collapsed={hidden} onFocus={() => reveal({x:b.x+b.w-145,y:hidden ? b.y+b.h-36 : b.y-14,w:145,h:28})} onToggle={() => setCollapsed(p.id, !hidden)} />
              </div>
            );
          })}
          <svg className="edges" width={W} height={H} aria-hidden="true">
            <defs>
              <marker id={`arrow-${rig}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head" />
              </marker>
              <marker id={`arrow-flow-${rig}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head flowing" />
              </marker>
            </defs>
            {drawnEdges.map((e) => {
              const g = routes.get(e);
              if (!g || !edgeInView(e)) return null;
              const flowing = !liveOnly && agents[e.from]?.state.key === 'working';
              const pulsing = e.enforced && (e.kind === 'delegates_to' || liveOnly) && !!pulses?.has(pulseKey(e.from, e.to));
              return (
                <g key={`${e.kind}-${e.from}-${e.to}`}>
                  <path
                    d={g.d}
                    className={`edge ${e.enforced ? 'enforced' : 'intent'}${flowing ? ' flowing' : ''}${pulsing ? ' handoff-pulse' : ''}${lit(e)}`}
                    data-testid={`edge-${e.from}-${e.to}`}
                    data-pulse={pulsing ? 'handoff' : undefined}
                    markerEnd={`url(#${flowing ? 'arrow-flow' : 'arrow'}-${rig})`}
                  />
                  {/* Work moving: dashes travel along the line (still dashes when motion is reduced). */}
                  {flowing && <path d={g.d} className="edge-flow" />}
                </g>
              );
            })}
          </svg>
          {workflowDraft && onWorkflow && <WorkflowGraph collapsed={collapsed} draft={workflowDraft} onOpen={onWorkflow} onReveal={reveal} readOnly agentBox={logos ? logoRouteBox : undefined} />}
          {drawnEdges.map((e, i) => {
            const g = spots.get(String(i));
            if (!g || !edgeInView(e)) return null;
            return (
              <span
                key={`l-${e.kind}-${e.from}-${e.to}`}
                className={`edge-label ${e.enforced ? 'enforced' : 'intent'}${g.wrap ? ' wrap' : ''}${lit(e)}`}
                data-testid={`edge-label-${e.from}-${e.to}`}
                style={{ left: g.x, top: g.y, maxWidth: g.maxWidth }}
                aria-hidden="true"
              >
                {(routes.get(e)?.merged.length ?? 1) > 1 ? `${routes.get(e)!.merged.length} arrows` : label(e)}
              </span>
            );
          })}
          {compiled.seats.map((s) => {
            const b = layout.seats[s.id];
            const a = agents[s.id];
            if (!b || !a || projection.hidden.has(s.id) || fold.hidden.has(s.id)) return null;
            if (!boxInView(view, b)) {
              const focusCard = () => {
                reveal(b);
                onSelect?.(s.id);
                requestAnimationFrame(() => viewportEl.current?.querySelector<HTMLElement>(`[data-agent="${CSS.escape(s.id)}"]`)?.focus({ preventScroll: true }));
              };
              return (
                <div key={s.id} className="card-slot card-stub" style={{ left: b.x, top: b.y, width: b.w, height: b.h }}>
                  <button type="button" aria-label={`${a.name} — ${a.state.word}`} onFocus={focusCard} onClick={() => onOpen(s.id)} />
                </div>
              );
            }
            return (
              <div key={s.id} className="card-slot" style={{ left: b.x, top: b.y, width: b.w, height: b.h }} onPointerEnter={() => setHover(s.id)} onPointerLeave={() => setHover((h) => (h === s.id ? null : h))}>
                {logos ? (
                  <LogoBox
                    a={a}
                    selected={selected === s.id}
                    picked={!!picked?.has(s.id)}
                    onOpen={() => onOpen(s.id)}
                    onFocusBox={() => {
                      reveal(b);
                      onSelect?.(s.id);
                    }}
                    motion={motion}
                    chips={chipsFor(s.id)}
                  />
                ) : (
                  <AgentBox
                    a={a}
                    selected={selected === s.id}
                    picked={!!picked?.has(s.id)}
                    onOpen={() => onOpen(s.id)}
                    onAnswer={() => onAnswer(s.id)}
                    onFocusBox={() => {
                      reveal(b);
                      onSelect?.(s.id);
                    }}
                    now={now}
                    components={chipsFor(s.id)}
                  />
                )}
              </div>
            );
          })}
          {([['more', fold.more], ['deeper', fold.deeper]] as const).flatMap(([kind, groups]) => [...groups].map(([at, ids]) => {
            const box = layout.seats[ids[0]];
            if (!box || projection.hidden.has(at)) return null;
            const name = agents[at]?.name ?? nameForId(at, names);
            return <HiddenAgents key={`${kind}-${at}`} box={box} ids={ids} kind={kind} relation={kind === 'more' ? `spawned by ${name}` : `below ${name}`} agents={agents} onOpen={onOpen} />;
          }))}
          {notStarted.filter(id => !projection.hidden.has(id)).map((id) => {
            const b = layout.seats[id];
            return (
              <div key={id} className="card-slot" style={{ left: b.x, top: b.y, width: b.w, height: b.h }}>
                <div className="agent-box st-planned placeholder">
                  <span className="box-main">{WORDS.notStartedAgent(nameForId(id, names))}</span>
                </div>
              </div>
            );
          })}
        </div>
      </div>
      <p className="sr-only" aria-live="polite" data-testid="handoff-announce">
        {compiled.edges
          .filter((e) => e.kind === 'delegates_to' && pulses?.has(pulseKey(e.from, e.to)))
          .map((e) => `${agents[e.from]?.name ?? nameForId(e.from, names)} handed work to ${agents[e.to]?.name ?? nameForId(e.to, names)}.`)
          .join(' ')}
      </p>
      {offCanvas.length > 0 && (
        <section className="off-canvas" aria-label="Not on the canvas">
          <h2>Not on the canvas</h2>
          <p>These agents ran in this team's last start but are no longer part of it. Their history is kept.</p>
          <ul>
            {offCanvas.map((s) => {
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
      )}
    </div>
  );
}
