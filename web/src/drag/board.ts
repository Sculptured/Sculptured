import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EditResult } from '../useTeamDraft';
import { moveAgents } from '../draft/bulk';
import { addAgent, moveAgent, moveGroup, placeAgent } from '../draft/edits';
import { agentBox, alignCard, clearOf, COL, drawnGroups, firstFreeSlot, overlaps, ROW } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { closest, dist, distinct, free, lerp, sticky, whenFar, withFloating, withSnapRadius, type Behavior, type Point, type Target } from './spec';

/**
 * The board's drags, declared as the states they can reach (see `spec.ts`). Every target is a
 * whole draft produced by the board's own edits (`moveAgent`, `addAgent`, `moveGroup`), so the
 * preview shows everything a drop would change: the group it joins grows, the one it left
 * shrinks, groups in the way make room.
 */

/** Board units: within this distance a card snaps onto a slot. */
export const SNAP = 36;
/** A slot that is previewed stays until another is this much closer. */
const STICK = 8;

type Draft = TeamDraft;
const unpack = (r: EditResult) => ('agents' in r ? { draft: r, reason: undefined } : r);

/** One agent put down: the draft it makes, the agent's id, or why it can't be. */
export type Placed = { draft: Draft; id: string; reason?: string };
/** Puts the agent at `at`, in `group` when given (null: no group), else in the group the drop point decides. */
export type Place = (d: Draft, at: Point, group?: string | null) => Placed;
/** A place to put an agent down, and the group it joins there (undefined: the drop point decides). */
export type Spot = { at: Point; group?: string | null };

/** Moving agent `id`; `keepGroup` (a running team) never changes its group. */
export const moving = (id: string, keepGroup: boolean): Place => (d, at, group) =>
  ({ ...(group === undefined ? moveAgent(d, id, at, keepGroup) : placeAgent(d, id, at, group, keepGroup)), id });
/** A new agent from the palette (`switchToStateAndFollow`: the drag follows a card that exists only in the targets). */
export const adding = (kind: AgentKind): Place => (d, at, group) => {
  const r = addAgent(d, kind, at);
  return group === undefined ? r : { ...placeAgent(r.draft, r.id, at, group), id: r.id };
};

/**
 * The target a placement makes, or null when it may not be offered: the edit refused it (a running
 * team's agent changing group), or the card would leave the canvas or land on another card.
 */
export function landing(r: Placed): Target<Draft> | null {
  if (r.reason) return null;
  const a = r.draft.agents.find((x) => x.id === r.id);
  if (!a || a.x < 0 || a.y < 0) return null;
  const box = agentBox(a);
  if (r.draft.agents.some((o) => o.id !== a.id && overlaps(agentBox(o), box))) return null;
  return { state: r.draft, at: { x: a.x, y: a.y }, key: `${groupOf(a)}@${a.x},${a.y}` };
}

/**
 * Where an agent can be put down without free placement: where it is (in its group), beside every
 * other card at the board's column and row spacing (joining that card's group), and the first free
 * place in each group. `skip`: cards not drawn (inside a collapsed group).
 */
export function slots(d: Draft, id: string | null, skip: ReadonlySet<string> = new Set()): Spot[] {
  const drawn = new Set(drawnGroups(d).map((g) => g.id));
  const self = d.agents.find((a) => a.id === id);
  const out: Spot[] = self ? [{ at: { x: self.x, y: self.y }, group: self.group_id }] : [];
  for (const o of d.agents) {
    if (o.id === id || skip.has(o.id)) continue;
    const group = drawn.has(groupOf(o)) ? groupOf(o) : undefined;
    for (const at of [{ x: o.x + COL, y: o.y }, { x: o.x - COL, y: o.y }, { x: o.x, y: o.y + ROW }, { x: o.x, y: o.y - ROW }]) out.push({ at, group });
  }
  for (const g of drawn) {
    const at = firstFreeSlot(d, g, id ?? undefined);
    if (at) out.push({ at, group: g });
  }
  return out.filter((s) => s.at.x >= 0 && s.at.y >= 0);
}

/** Every valid state the slots produce, one per distinct landing. */
export const slotTargets = (d: Draft, id: string | null, place: Place, skip?: ReadonlySet<string>): Target<Draft>[] =>
  distinct(slots(d, id, skip).map((s) => landing(place(d, s.at, s.group))));

/**
 * Free placement at `p` (lined up with a nearby card when within reach). Over another card it is the
 * nearest clear place instead, like `vary` under a constraint. Null when the edit would put the
 * card somewhere else (it is then one of the slots, not a free placement).
 */
export function freeTarget(d: Draft, id: string | null, place: Place, p: Point): Target<Draft> | null {
  const at = (q: Point) => {
    const t = landing(place(d, q));
    return t && dist(t.at, q) < 1 ? t : null;
  };
  const q = alignCard(d, id ?? '', p);
  const exact = at(q);
  if (exact) return exact;
  const clear = clearOf(q, d.agents.filter((a) => a.id !== id).map(agentBox));
  return clear === q ? null : at(clear);
}

/**
 * Dragging one agent: `closest(slots).whenFar(free).withFloating().withSnapRadius()`. The card
 * follows the pointer, the board previews where it lands, and near a slot it is drawn toward it.
 */
export function agentDrag(d: Draft, id: string | null, place: Place, skip?: ReadonlySet<string>): Behavior<Draft> {
  const near = closest(slotTargets(d, id, place, skip), STICK);
  const far = free((p) => freeTarget(d, id, place, p));
  return withSnapRadius(withFloating(whenFar(near, far, SNAP)), SNAP);
}

/** Dragging a group: it goes as far toward the pointer as it can without covering another group. */
export function groupDrag(d: Draft, id: string): Behavior<Draft> {
  const g = d.groups.find((x) => x.id === id);
  return free((p) => {
    if (!g) return null;
    const state = moveGroup(d, id, p.x - g.x, p.y - g.y);
    const m = state.groups.find((x) => x.id === id)!;
    return { state, at: { x: m.x, y: m.y }, key: `${m.x},${m.y}` };
  });
}

/**
 * Dragging several selected agents together (`anchor` is the card under the pointer). A spot where
 * one would land on a card left behind, or where the move is refused, is skipped: the preview stays
 * at the last valid one.
 */
export function selectionDrag(d: Draft, ids: readonly string[], anchor: string, keepGroup: boolean): Behavior<Draft> {
  const a0 = d.agents.find((a) => a.id === anchor);
  const picked = new Set(ids);
  return sticky(
    free((p) => {
      if (!a0) return null;
      const { draft, reason } = unpack(moveAgents(d, [...ids], p.x - a0.x, p.y - a0.y, keepGroup));
      if (reason) return null;
      const moved = draft.agents.filter((a) => picked.has(a.id)).map(agentBox);
      const rest = draft.agents.filter((a) => !picked.has(a.id)).map(agentBox);
      if (moved.some((m) => rest.some((o) => overlaps(m, o)))) return null;
      const a = draft.agents.find((x) => x.id === anchor)!;
      return { state: draft, at: { x: a.x, y: a.y }, key: `${a.x},${a.y}` };
    }),
  );
}

/**
 * The board between two states, for animation: cards and groups in both move `t` of the way from
 * `a` to `b`; everything else is `b`'s. `pin` is drawn where `b` has it (the card under the pointer).
 */
export function lerpDraft(a: Draft, b: Draft, t: number, pin: string | null = null): Draft {
  if (a === b) return b;
  const from = new Map(a.agents.map((x) => [x.id, x]));
  const boxes = new Map(a.groups.map((g) => [g.id, g]));
  return {
    ...b,
    agents: b.agents.map((x) => {
      const o = from.get(x.id);
      return !o || x.id === pin ? x : { ...x, x: lerp(o.x, x.x, t), y: lerp(o.y, x.y, t) };
    }),
    groups: b.groups.map((g) => {
      const o = boxes.get(g.id);
      return o ? { ...g, x: lerp(o.x, g.x, t), y: lerp(o.y, g.y, t), w: lerp(o.w, g.w, t), h: lerp(o.h, g.h, t) } : g;
    }),
  };
}

/** The largest distance any card or group edge still has to travel from `a` to `b`. */
export function gap(a: Draft, b: Draft): number {
  let m = 0;
  const from = new Map(a.agents.map((x) => [x.id, x]));
  for (const x of b.agents) {
    const o = from.get(x.id);
    if (o) m = Math.max(m, Math.abs(o.x - x.x), Math.abs(o.y - x.y));
  }
  const boxes = new Map(a.groups.map((g) => [g.id, g]));
  for (const g of b.groups) {
    const o = boxes.get(g.id);
    if (o) m = Math.max(m, Math.abs(o.x - g.x), Math.abs(o.y - g.y), Math.abs(o.w - g.w), Math.abs(o.h - g.h));
  }
  return m;
}

/** `d` with agent `id` drawn at `p` (the floating card). */
export const drawnAt = (d: Draft, id: string, p: Point): Draft => ({
  ...d,
  agents: d.agents.map((a) => (a.id === id ? { ...a, x: p.x, y: p.y } : a)),
});
