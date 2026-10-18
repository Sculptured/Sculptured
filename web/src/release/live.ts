import type { CompiledRig } from "../generated/CompiledRig";
import type { EventRecord } from "../generated/EventRecord";
import type { SeatView } from "../generated/SeatView";
import type { Layout } from "../layout";
import { communicationStats } from "../collaboration/analytics";
import { parentForest, treeLayout, subtreeIds } from "./treeLayout";
import { computeLayout, withLiveChildren } from "../layout";
export function liveProjection(
  compiled: CompiledRig,
  seats: Record<string, SeatView>,
  events: EventRecord[],
  given?: Layout,
  children: { parent: string; child: string }[] = [],
  subtree?: string,
) {
  const scope = subtree ? subtreeIds(compiled, children, subtree) : null;
  if (scope) {
    compiled = {...compiled,seats:compiled.seats.filter(s=>scope.has(s.id)),edges:compiled.edges.filter(e=>scope.has(e.from)&&scope.has(e.to))};
    // Scope is an explicit user navigation; give this smaller tree its own compact coordinates.
    given = undefined;
  }
  // An agent that needs you or failed stays on the board: its card carries the reason.
  const live = compiled.seats.filter((s) =>
    ["running", "launching", "stopping", "attention", "failed"].includes(seats[s.id]?.lifecycle),
  );
  const ids = new Set(live.map((s) => s.id));
  const boundaries = new Map<string, number>();
  for (const e of events)
    if (e.subject.kind === "seat" && e.body.type === "tenure-started")
      boundaries.set(
        e.subject.id,
        Math.max(boundaries.get(e.subject.id) ?? 0, e.seq),
      );
  const current = events.filter(
    (e) =>
      e.subject.kind !== "seat" || e.seq >= (boundaries.get(e.subject.id) ?? 0),
  );
  const stats = communicationStats(current);
  const messages = stats.rows;
  const edges: CompiledRig["edges"] = [];
  const labels: Record<string, string> = {};
  const counts = new Map<string, number>();
  for (const m of messages)
    for (const from of m.from) {
      if (
        !ids.has(from) ||
        !ids.has(m.to) ||
        m.event.seq < (boundaries.get(from) ?? 0)
      )
        continue;
      const key = `${from}→${m.to}`;
      if (!counts.has(key))
        edges.push({
          from,
          to: m.to,
          kind: "collaborates_with",
          enforced: true,
        });
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  for (const [key, count] of counts)
    labels[key] = `${count} ${count === 1 ? "message" : "messages"} observed`;
  for (const e of current)
    if (e.body.type === "handed-over" && e.subject.kind === "seat")
      for (const source of e.body.from) {
        const to = e.subject.id,
          from = source.seat;
        if (
          ids.has(from) &&
          ids.has(to) &&
          e.seq >= (boundaries.get(from) ?? 0) &&
          !edges.some((edge) => edge.from === from && edge.to === to)
        ) {
          edges.push({ from, to, kind: "delegates_to", enforced: true });
          labels[`${from}→${to}`] = "Handoff recorded";
        }
      }
  const family = parentForest(compiled, children);
  // Parentage is a downward hierarchy arrow. Direct family traffic stays in Communications;
  // drawing a second return arrow here would obscure which node is the parent.
  for (const {parent, child} of family) {
    for (let i=edges.length-1;i>=0;i--) {
      const e=edges[i];
      if ((e.from===parent && e.to===child) || (e.from===child && e.to===parent)) edges.splice(i,1);
    }
    delete labels[`${child}→${parent}`];
    delete labels[`${parent}→${child}`];
    if (ids.has(parent) && ids.has(child)) {
      edges.push({from:parent,to:child,kind:"spawned_by",enforced:true});
      labels[`${parent}→${child}`] = "";
    }
  }
  const groups = new Set(live.map((s) => s.pod));
  const base = given ? withLiveChildren(given, compiled) : computeLayout(compiled);
  const layout = treeLayout(base, compiled, family);
  return {
    compiled: {
      ...compiled,
      seats: live,
      pods: compiled.pods.filter((p) => groups.has(p.id)),
      edges,
    },
    layout: {
      ...layout,
      seats: Object.fromEntries(
        Object.entries(layout.seats).filter(([id]) => ids.has(id)),
      ),
      pods: Object.fromEntries(
        Object.entries(layout.pods).filter(([id]) => groups.has(id)),
      ),
    },
    labels,
    unknown: compiled.seats.filter((s) => seats[s.id]?.lifecycle === "stale")
      .length,
  };
}

type Projection = ReturnType<typeof liveProjection>;

/** What the graph draws from `compiled`: the agents, groups and arrows (not message counts). */
const drawnCompiled = (c: Projection['compiled']) =>
  JSON.stringify([c.seats.map((s) => s.id), c.pods.map((p) => p.id), c.edges.map((e) => [e.from, e.to, e.kind])]);

/**
 * `next`, reusing `prev`'s compiled team and layout while nothing drawn has changed: live events
 * arrive many times a second, and new objects would make the graph route every arrow again.
 */
export function stableProjection(prev: Projection | null, next: Projection): Projection {
  if (!prev) return next;
  const compiled = drawnCompiled(prev.compiled) === drawnCompiled(next.compiled) ? prev.compiled : next.compiled;
  const layout = JSON.stringify(prev.layout) === JSON.stringify(next.layout) ? prev.layout : next.layout;
  return compiled === next.compiled && layout === next.layout ? next : { ...next, compiled, layout };
}
