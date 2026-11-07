import { useMemo, useRef } from 'react';
import type { CompiledRig } from '../generated/CompiledRig';
import type { CompiledEdge } from '../generated/CompiledEdge';
import type { TeamDraft } from '../generated/TeamDraft';
import { computeLayout, withLiveChildren, type Layout } from '../layout';
import { seatIdFor } from '../draft/ids';
import { EDGE_WORDS, NOT_ENFORCED } from '../vocab';
import { foldChildren } from './childFold';
import { projectGroups, routeGroupEdges } from './groupVisibility';
import { routeEdges } from './edgeGeometry';
import { placeLabels } from './labels';
import { logoRouteBoxes, useBoardStyle } from './boardStyle';
import { workflowBounds } from '../workflow/WorkflowGraph';

export function edgeLabel(e: CompiledEdge): string {
  return `${EDGE_WORDS[e.kind]}${e.enforced ? '' : ` · ${NOT_ENFORCED}`}`;
}

type Inputs = {
  compiled: CompiledRig;
  given?: Layout;
  liveOnly?: boolean;
  draft?: TeamDraft;
  collapsed: Set<string>;
  observedLabels?: Record<string, string>;
};

type Draft = TeamDraft;

/** The draft with each agent and group moved to where the board draws it, so the workflow graph lines up. */
function placedDraft(draft: Draft | undefined, layout: Layout): Draft | undefined {
  if (!draft?.workflow) return undefined;
  return {
    ...draft,
    agents: draft.agents.map(a => { const b = layout.seats[seatIdFor(draft, a.id) ?? '']; return b ? { ...a, x: b.x, y: b.y } : a; }),
    groups: draft.groups.map(g => ({ ...g, ...layout.pods[g.id] })),
  };
}

/** Which group each seat sits in: its draft group, else its compiled pod. */
function groupBySeat(draft: Draft | undefined, compiled: CompiledRig) {
  return Object.fromEntries([...(draft?.agents.map(a => [seatIdFor(draft, a.id)!, a.group_id ?? 'team']) ?? []), ...compiled.seats.map(s => [s.id, s.pod])]);
}

/** Everything drawn, including connection arcs above or below the boxes. Visibility is a lens over the saved
 * drawing, so toggles never trigger auto-fit. */
function drawnBounds(edges: CompiledEdge[], layout: Layout, workflowDraft: Draft | undefined, label: (e: CompiledEdge) => string) {
  const fitRoutes = routeEdges(edges, layout.seats);
  const fitLabels = placeLabels(edges.flatMap((e, i) => {
    const geom = fitRoutes.get(e); return geom && label(e) ? [{ key: String(i), geom, text: label(e) }] : [];
  }), Object.values(layout.seats));
  const mids = [...fitRoutes.values()].map((g) => g.mid.y);
  const controls = [...fitRoutes.values()].flatMap(g => g.pts);
  const tags = [...fitLabels.values()];
  const boxes = [...Object.values(layout.seats), ...Object.values(layout.pods), ...(workflowDraft ? workflowBounds(workflowDraft) : [])];
  return {
    x0: Math.min(0, ...boxes.map((b) => b.x), ...controls.map(p => p.x - 12)),
    y0: Math.min(0, ...boxes.map((b) => b.y), ...mids.map((y) => y - 16), ...tags.map((t) => t.y - t.h / 2 - 4)),
    x1: Math.max(workflowDraft ? 0 : layout.width, ...boxes.map((b) => b.x + b.w), ...controls.map(p => p.x + 12)),
    y1: Math.max(workflowDraft ? 0 : layout.height, ...boxes.map((b) => b.y + b.h), ...mids.map((y) => y + 16), ...tags.map((t) => t.y + t.h / 2 + 4)),
  };
}

/** Where everything on the live graph goes: layout, groups, folded agents, arrow routes, label spots and the fit bounds. */
export function useGraphGeometry({ compiled, given, liveOnly, draft, collapsed, observedLabels }: Inputs) {
  const label = (e: CompiledEdge) => observedLabels?.[`${e.from}→${e.to}`] ?? edgeLabel(e);
  // Labels are read when routes or bounds are recomputed, not on every change to their text.
  const labelRef = useRef(label);
  labelRef.current = label;
  const layout = useMemo(() => given ? (liveOnly ? given : withLiveChildren(given, compiled)) : computeLayout(compiled), [given, compiled, liveOnly]);
  const workflowDraft = useMemo(() => placedDraft(draft, layout), [draft, layout]);
  const projection = useMemo(() => projectGroups(layout.seats, groupBySeat(draft, compiled), layout.pods, collapsed), [layout, compiled, collapsed, draft]);
  // Many or deep spawned agents fold into "+N more" / "+N deeper"; their saved boxes stay put.
  const fold = useMemo(() => foldChildren(compiled), [compiled]);
  const drawnEdges = useMemo(() => compiled.edges.filter((e) => !fold.hidden.has(e.from) && !fold.hidden.has(e.to)), [compiled.edges, fold]);
  // Logos style: same slots, but the arrows end at the logo, not at the slot's edge.
  const logos = useBoardStyle() === 'logos';
  const routes = useMemo(() => routeGroupEdges(drawnEdges, logos ? { ...projection, boxes: logoRouteBoxes(projection.boxes) } : projection), [drawnEdges, projection, logos]);
  // Labels sit beside their curves, never on a curve, a card or another label.
  const spots = useMemo(() => placeLabels(drawnEdges.flatMap((e, i) => {
    const g = routes.get(e);
    const text = g && (g.merged.length > 1 ? `${g.merged.length} arrows` : labelRef.current(e));
    return g && text ? [{ key: String(i), geom: g, text }] : [];
  }), Object.values(projection.boxes)), [drawnEdges, routes, projection]);
  const bounds = useMemo(() => drawnBounds(compiled.edges, layout, workflowDraft, labelRef.current), [compiled.edges, layout, workflowDraft]);
  return { label, layout, workflowDraft, projection, fold, drawnEdges, logos, routes, spots, bounds };
}
