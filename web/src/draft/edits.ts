import type { AgentAdvanced } from '../generated/AgentAdvanced';
import type { AgentKind } from '../generated/AgentKind';
import type { ConnectionKind } from '../generated/ConnectionKind';
import type { DraftAgent } from '../generated/DraftAgent';
import type { DraftGroup } from '../generated/DraftGroup';
import type { TeamDraft } from '../generated/TeamDraft';
import { CARD_H, CARD_W } from '../layout';
import { withoutAgents } from './removal';
import { CONNECTION_WORDS } from '../vocab';
import {
  type Box,
  ceilGrid,
  drawnGroups,
  firstFreeSlot,
  floorGrid,
  GRID,
  GROUP_GAP,
  GROUP_HEAD,
  GROUP_PAD,
  clearLoose,
  groupAt,
  looseAllowed,
  MAIN_GROUP_LABEL,
  MARGIN,
  nearestGroup,
  overlaps,
  snap,
  type Point,
} from './geometry';
import { DEFAULT_GROUP_ID, groupOf, newId, nextAgentName, slug } from './ids';

export type Selection = { kind: 'agent' | 'group' | 'connection'; id: string };

/** Shown when a drop would move an agent of a running team into another group. */
export const RUNNING_GROUP_REASON = "An agent can't change group while the team is running.";

const DEFAULT_ADVANCED: AgentAdvanced = { model: null, permission: 'unrestricted', resume: 'resume-if-possible' };

const nameOf = (d: TeamDraft, id: string) => d.agents.find((a) => a.id === id)?.name.trim() || 'This agent';

const alreadyConnected = (d: TeamDraft, from: string, to: string, kind: ConnectionKind) =>
  `${nameOf(d, from)} already ${CONNECTION_WORDS[kind]} ${nameOf(d, to)}.`;

export function selectionExists(d: TeamDraft, s: Selection | null): Selection | null {
  if (!s) return null;
  const found =
    s.kind === 'agent'
      ? d.agents.some((a) => a.id === s.id)
      : s.kind === 'group'
        ? d.groups.some((g) => g.id === s.id)
        : d.connections.some((c) => c.id === s.id);
  return found ? s : null;
}

/**
 * How far `b` must move along one axis to clear every box in `placed`: in `dir` (+1 right or
 * down, -1 left or up), in grid steps. Null when moving left or up would leave the canvas.
 */
function clearance(b: Box, placed: Box[], axis: 'x' | 'y', dir: 1 | -1): number | null {
  let off = 0;
  for (;;) {
    const moved = axis === 'x' ? { ...b, x: b.x + off } : { ...b, y: b.y + off };
    const hit = placed.find((p) => overlaps(moved, p));
    if (!hit) return off;
    // Strictly grows in `dir`, and a box once cleared is never hit again, so this ends.
    if (dir === 1) off = axis === 'x' ? ceilGrid(hit.x + hit.w + GROUP_GAP - b.x) : ceilGrid(hit.y + hit.h + GROUP_GAP - b.y);
    else off = axis === 'x' ? floorGrid(hit.x - GROUP_GAP - (b.x + b.w)) : floorGrid(hit.y - GROUP_GAP - (b.y + b.h));
    if ((axis === 'x' ? b.x : b.y) + off < 0) return null;
  }
}

/** Away from the anchor along one axis: left or up when there is room, else right or down. */
function away(b: Box, anchor: Box | undefined, placed: Box[], axis: 'x' | 'y'): number {
  const centre = (x: Box) => (axis === 'x' ? x.x + x.w / 2 : x.y + x.h / 2);
  if (anchor && centre(b) < centre(anchor)) {
    const back = clearance(b, placed, axis, -1);
    if (back !== null) return back;
  }
  return clearance(b, placed, axis, 1)!;
}

/**
 * Makes room so that no two drawn group boxes overlap. `anchor` (the group the edit was about)
 * stays put. A group that overlaps an already placed one moves away from the anchor along the
 * shorter axis: left or up when the canvas has room there (x and y stay at 0 or more), right or
 * down otherwise. It takes its agents with it. Groups that overlap nothing never move.
 */
function separate(d: TeamDraft, anchor?: string): TeamDraft {
  const gs = drawnGroups(d);
  if (!gs.some((a, i) => gs.some((b, j) => j > i && overlaps(a, b)))) return d;
  const order = [...gs].sort((a, b) => (a.id === anchor ? -1 : b.id === anchor ? 1 : a.x - b.x || a.y - b.y));
  const anchorBox = gs.find((g) => g.id === anchor);
  const placed: Box[] = [];
  const shift = new Map<string, Point>();
  for (const g of order) {
    const hit = placed.some((p) => overlaps(g, p));
    const dx = hit ? away(g, anchorBox, placed, 'x') : 0;
    const dy = hit ? away(g, anchorBox, placed, 'y') : 0;
    const off = !hit ? { x: 0, y: 0 } : Math.abs(dy) < Math.abs(dx) ? { x: 0, y: dy } : { x: dx, y: 0 };
    placed.push({ x: g.x + off.x, y: g.y + off.y, w: g.w, h: g.h });
    if (off.x || off.y) shift.set(g.id, off);
  }
  return {
    ...d,
    groups: d.groups.map((g) => {
      const off = shift.get(g.id);
      return off ? { ...g, x: g.x + off.x, y: g.y + off.y } : g;
    }),
    agents: d.agents.map((a) => {
      const off = shift.get(groupOf(a));
      return off ? { ...a, x: a.x + off.x, y: a.y + off.y } : a;
    }),
  };
}

/**
 * Grows each group so it contains its agents (an agent without a group belongs to the main
 * group). Groups never shrink. A grown group never covers another: overlapped groups make room
 * (see `separate`), and `anchor` is the group that keeps its place.
 */
export function fitGroups(d: TeamDraft, anchor?: string): TeamDraft {
  if (d.groups.length === 0) return d;
  const groups = d.groups.map((g) => {
    let { x, y, w, h } = g;
    for (const a of d.agents) {
      if (groupOf(a) !== g.id) continue;
      const left = Math.min(x, Math.max(0, a.x - GROUP_PAD));
      const top = Math.min(y, Math.max(0, a.y - GROUP_PAD - GROUP_HEAD));
      const right = Math.max(x + w, a.x + CARD_W + GROUP_PAD);
      const bottom = Math.max(y + h, a.y + CARD_H + GROUP_PAD);
      x = left;
      y = top;
      w = right - left;
      h = bottom - top;
    }
    return x === g.x && y === g.y && w === g.w && h === g.h ? g : { ...g, x, y, w, h };
  });
  return clearLoose(separate(groups.every((g, i) => g === d.groups[i]) ? d : { ...d, groups }, anchor));
}


/**
 * Adds an agent at `at`. A drop inside a group joins it there; a drop outside every group stays
 * outside them (unless the main group is drawn: then it takes the nearest group's first free slot).
 */
export function addAgent(d: TeamDraft, kind: AgentKind, at: Point): { draft: TeamDraft; id: string } {
  const name = nextAgentName(d, kind);
  const id = newId(slug(name, 'agent'), new Set(d.agents.map((a) => a.id)));
  let pos = { x: snap(at.x), y: snap(at.y) };
  let group_id: string | null = null;
  if (drawnGroups(d).length > 0) {
    const center = { x: pos.x + CARD_W / 2, y: pos.y + CARD_H / 2 };
    group_id = groupAt(d, center);
    if (group_id === null && !looseAllowed(d)) {
      group_id = nearestGroup(d, center)!;
      pos = firstFreeSlot(d, group_id)!;
    }
  }
  const agent: DraftAgent = { id, group_id, name, kind, job: '', folder: null, ...pos, advanced: { ...DEFAULT_ADVANCED } };
  return { draft: fitGroups({ ...d, agents: [...d.agents, agent] }, group_id ?? undefined), id };
}

/**
 * Moves an agent. Dropping its centre on a drawn group moves it into that group; a drop outside
 * every group takes it out of its group, to where it was dropped. (While the main group is drawn,
 * an agent cannot be outside every group: it takes its own group's first free slot instead.)
 * `keepGroup` (the team is running) never changes the group, because that is structural: a drop
 * on another group is refused with a reason, and a drop outside keeps the card in its group.
 */
export function moveAgent(d: TeamDraft, id: string, to: Point, keepGroup = false): { draft: TeamDraft; reason?: string } {
  const a = d.agents.find((x) => x.id === id);
  if (!a) return { draft: d };
  const pos = { x: snap(to.x), y: snap(to.y) };
  if (drawnGroups(d).length === 0) return placeAgent(d, id, pos, a.group_id);
  const target = groupAt(d, { x: pos.x + CARD_W / 2, y: pos.y + CARD_H / 2 });
  if (target !== null) return placeAgent(d, id, pos, target, keepGroup);
  if (looseAllowed(d) && (a.group_id === null || !keepGroup)) return placeAgent(d, id, pos, null);
  return placeAgent(d, id, firstFreeSlot(d, groupOf(a), id) ?? pos, a.group_id);
}

/**
 * Puts an agent at `to` in group `group` (null: no group). The group grows to hold it and the one
 * it left shrinks. Membership that does not change keeps its spelling (an agent without a group
 * stays so). `keepGroup` (the team is running) refuses a change of group.
 */
export function placeAgent(d: TeamDraft, id: string, to: Point, group: string | null, keepGroup = false): { draft: TeamDraft; reason?: string } {
  const a = d.agents.find((x) => x.id === id);
  if (!a) return { draft: d };
  const home = groupOf(a);
  const changes = groupOf({ group_id: group }) !== home;
  if (keepGroup && changes) return { draft: d, reason: RUNNING_GROUP_REASON };
  const group_id = changes ? group : a.group_id;
  const moved = { ...d, agents: d.agents.map((x) => (x.id === id ? { ...x, x: snap(to.x), y: snap(to.y), group_id } : x)) };
  return { draft: fitGroups(changes ? shrinkLeft(moved, home) : moved, group_id ?? undefined) };
}


/** How far `box` may travel along one axis (`move` > 0 forward, < 0 back) before touching a box in its way. */
function clampAxis(box: Box, others: Box[], move: number, axis: 'x' | 'y'): number {
  const [pos, len, cross, crossLen] = axis === 'x' ? (['x', 'w', 'y', 'h'] as const) : (['y', 'h', 'x', 'w'] as const);
  let m = move;
  for (const o of others) {
    if (!(box[cross] < o[cross] + o[crossLen] && o[cross] < box[cross] + box[crossLen])) continue;
    if (m > 0 && o[pos] >= box[pos] + box[len]) m = Math.min(m, Math.max(0, floorGrid(o[pos] - GROUP_GAP - (box[pos] + box[len]))));
    if (m < 0 && o[pos] + o[len] <= box[pos]) m = Math.max(m, -Math.max(0, floorGrid(box[pos] - (o[pos] + o[len]) - GROUP_GAP)));
  }
  return m;
}

/**
 * Moves a group and its agents together, on the grid. It never leaves the canvas and stops short
 * of another group's box instead of covering it.
 */
export function moveGroup(d: TeamDraft, id: string, dx: number, dy: number): TeamDraft {
  const g = d.groups.find((x) => x.id === id);
  if (!g) return d;
  const members = d.agents.filter((a) => groupOf(a) === id);
  const step = (n: number) => Math.round(n / GRID) * GRID;
  // Rounded up to the grid so a clamped move keeps every card on it.
  const wantX = Math.max(step(dx), ceilGrid(-Math.min(g.x, ...members.map((a) => a.x))));
  const wantY = Math.max(step(dy), ceilGrid(-Math.min(g.y, ...members.map((a) => a.y))));
  const others = drawnGroups(d).filter((o) => o.id !== id);
  // Horizontal first, against groups sharing its rows; then vertical from there, against groups
  // sharing its columns. Neither step can end on a group that was clear before.
  const mx = clampAxis(g, others, wantX, 'x');
  const my = clampAxis({ ...g, x: g.x + mx }, others, wantY, 'y');
  if (mx === 0 && my === 0) return d;
  return clearLoose(separate(shifted(d, id, mx, my), id));
}

function shifted(d: TeamDraft, id: string, mx: number, my: number): TeamDraft {
  return {
    ...d,
    groups: d.groups.map((x) => (x.id === id ? { ...x, x: x.x + mx, y: x.y + my } : x)),
    agents: d.agents.map((a) => (groupOf(a) === id ? { ...a, x: a.x + mx, y: a.y + my } : a)),
  };
}

/** Takes one agent out of its group: its group shrinks and the card moves out from under the box. */
export function takeOut(d: TeamDraft, id: string): TeamDraft {
  const a = d.agents.find((x) => x.id === id);
  if (!a || a.group_id === null || !looseAllowed(d)) return d;
  const moved = { ...d, agents: d.agents.map((x) => (x.id === id ? { ...x, group_id: null } : x)) };
  return fitGroups(shrinkLeft(moved, a.group_id));
}

/**
 * Removes a group's box and keeps its agents, now outside every group (moved out from under any
 * other group's box). Their seat ids change, so the team must not be running.
 */
export function ungroup(d: TeamDraft, id: string): TeamDraft {
  if (!d.groups.some((g) => g.id === id)) return d;
  const agents = d.agents.map((a) => (groupOf(a) === id ? { ...a, group_id: null } : a));
  return fitGroups({ ...d, groups: d.groups.filter((g) => g.id !== id), agents });
}

/**
 * Deletes an agent (with its connections), a connection, or a group with all its agents and workflow references. When a delete
 * leaves only the main group, it becomes implicit again: no group, and its agents have none,
 * which keeps every seat id.
 */
export function removeElement(d: TeamDraft, s: Selection): { draft: TeamDraft; reason?: string } {
  if (s.kind === 'connection') return { draft: { ...d, connections: d.connections.filter((c) => c.id !== s.id) } };
  const removed = new Set(d.agents.filter(a => s.kind === 'agent' ? a.id === s.id : groupOf(a) === s.id).map(a => a.id));
  d = withoutAgents(d, removed);
  if (s.kind === 'agent') return {draft:d};
  const groups = d.groups.filter((g) => g.id !== s.id);
  // A lone main group folds away unless you renamed it: then it stays drawn with its label.
  if (groups.length === 1 && groups[0].id === DEFAULT_GROUP_ID && groups[0].label.trim() === MAIN_GROUP_LABEL) {
    return { draft: { ...d, groups: [], agents: d.agents.map((a) => (a.group_id === DEFAULT_GROUP_ID ? { ...a, group_id: null } : a)) } };
  }
  return { draft: { ...d, groups } };
}

export function connect(
  d: TeamDraft,
  from: string,
  to: string,
  kind: ConnectionKind = 'hands-work-to',
): { draft: TeamDraft; id: string | null; reason?: string } {
  const has = (id: string) => d.agents.some((a) => a.id === id);
  if (!has(from) || !has(to)) return { draft: d, id: null, reason: 'Both agents must be on this team.' };
  if (from === to) return { draft: d, id: null, reason: 'An agent cannot hand work to itself.' };
  if (d.connections.some((c) => c.from === from && c.to === to && c.kind === kind)) {
    return { draft: d, id: null, reason: alreadyConnected(d, from, to, kind) };
  }
  const id = newId('c', new Set(d.connections.map((c) => c.id)));
  return { draft: { ...d, connections: [...d.connections, { id, from, to, kind }] }, id };
}

/** Reconnect an arrow without changing its identity or type. */
export function reconnect(d: TeamDraft, id: string, end: 'from' | 'to', agent: string): { draft: TeamDraft; reason?: string } {
  const current = d.connections.find(c => c.id === id);
  if (!current || current[end] === agent) return { draft: d };
  const next = { ...current, [end]: agent };
  if (!d.agents.some(a => a.id === agent)) return { draft: d, reason: 'Both agents must be on this team.' };
  if (next.from === next.to) return { draft: d, reason: 'An agent cannot hand work to itself.' };
  if (d.connections.some(c => c.id !== id && c.from === next.from && c.to === next.to && c.kind === next.kind)) return { draft: d, reason: alreadyConnected(d, next.from, next.to, next.kind) };
  return { draft: { ...d, connections: d.connections.map(c => c.id === id ? next : c) } };
}

/** Changes a connection's type, unless the same two agents already have one of that type. */
export function setConnectionKind(d: TeamDraft, id: string, kind: ConnectionKind): { draft: TeamDraft; reason?: string } {
  const c = d.connections.find((x) => x.id === id);
  if (!c || c.kind === kind) return { draft: d };
  if (d.connections.some((x) => x.id !== id && x.from === c.from && x.to === c.to && x.kind === kind)) {
    return { draft: d, reason: alreadyConnected(d, c.from, c.to, kind) };
  }
  return { draft: { ...d, connections: d.connections.map((x) => (x.id === id ? { ...x, kind } : x)) } };
}

function around(agents: DraftAgent[]): Omit<DraftGroup, 'id' | 'label'> {
  if (agents.length === 0) return { x: 0, y: 0, w: GROUP_PAD * 2 + CARD_W, h: GROUP_HEAD + GROUP_PAD * 2 + CARD_H };
  const x = Math.max(0, Math.min(...agents.map((a) => a.x)) - GROUP_PAD);
  const y = Math.max(0, Math.min(...agents.map((a) => a.y)) - GROUP_PAD - GROUP_HEAD);
  const right = Math.max(...agents.map((a) => a.x + CARD_W)) + GROUP_PAD;
  const bottom = Math.max(...agents.map((a) => a.y + CARD_H)) + GROUP_PAD;
  return { x, y, w: right - x, h: bottom - y };
}

/**
 * Adds an empty group at `at`, or to the right of everything. Agents outside every group stay
 * outside: a group holds only the agents you put in it.
 */
export function addGroup(d: TeamDraft, at?: Point): { draft: TeamDraft; id: string } {
  const right = Math.max(0, ...d.groups.map((g) => g.x + g.w), ...d.agents.map((a) => a.x + CARD_W));
  const id = newId('group', new Set(d.groups.map((g) => g.id)));
  const labels = new Set(d.groups.map((g) => g.label.trim().toLowerCase()));
  let n = drawnGroups(d).length + 1;
  while (labels.has(`group ${n}`)) n++;
  const group: DraftGroup = {
    id,
    label: `Group ${n}`,
    x: snap(at?.x ?? right + 60),
    y: snap(at?.y ?? MARGIN),
    w: GROUP_PAD * 2 + CARD_W,
    h: GROUP_HEAD + GROUP_PAD * 2 + CARD_H,
  };
  return { draft: fitGroups({ ...d, groups: [...d.groups, group] }, id), id };
}

/**
 * The group an agent just left shrinks to fit the agents that stay (an empty group keeps room
 * for one card), so no empty space is left where the agent was. The box only ever gets smaller
 * and stays where it was, so it cannot cover another group.
 */
function shrinkLeft(d: TeamDraft, groupId: string): TeamDraft {
  const g = d.groups.find((x) => x.id === groupId);
  if (!g) return d;
  const members = d.agents.filter((a) => groupOf(a) === groupId);
  const t = members.length > 0 ? around(members) : { x: g.x, y: g.y, w: GROUP_PAD * 2 + CARD_W, h: GROUP_HEAD + GROUP_PAD * 2 + CARD_H };
  const x = Math.max(g.x, t.x);
  const y = Math.max(g.y, t.y);
  const box = { x, y, w: Math.min(g.x + g.w, t.x + t.w) - x, h: Math.min(g.y + g.h, t.y + t.h) - y };
  if (box.x === g.x && box.y === g.y && box.w === g.w && box.h === g.h) return d;
  return { ...d, groups: d.groups.map((x2) => (x2.id === groupId ? { ...x2, ...box } : x2)) };
}

/** Keyboard path for dragging into a group: the agent takes the first free slot inside it. */
export function moveToGroup(d: TeamDraft, agentId: string, groupId: string): TeamDraft {
  const slot = firstFreeSlot(d, groupId, agentId);
  const a = d.agents.find((x) => x.id === agentId);
  if (!slot || !a) return d;
  const from = groupOf(a);
  const agents = d.agents.map((x) => (x.id === agentId ? { ...x, group_id: from === groupId ? x.group_id : groupId, x: slot.x, y: slot.y } : x));
  const moved = { ...d, agents };
  return fitGroups(from === groupId ? moved : shrinkLeft(moved, from), groupId);
}

const patchAgent = (d: TeamDraft, id: string, patch: Partial<DraftAgent>): TeamDraft => ({
  ...d,
  agents: d.agents.map((a) => (a.id === id ? { ...a, ...patch } : a)),
});

export const renameTeam = (d: TeamDraft, name: string): TeamDraft => ({ ...d, name });
export const renameAgent = (d: TeamDraft, id: string, name: string) => patchAgent(d, id, { name });
export const setJob = (d: TeamDraft, id: string, job: string) => patchAgent(d, id, { job });
export const setFolder = (d: TeamDraft, id: string, folder: string | null) => patchAgent(d, id, { folder });
export const setAdvanced = (d: TeamDraft, id: string, patch: Partial<AgentAdvanced>): TeamDraft => ({
  ...d,
  agents: d.agents.map((a) => (a.id === id ? { ...a, advanced: { ...a.advanced, ...patch } } : a)),
});
export const renameGroup = (d: TeamDraft, id: string, label: string): TeamDraft => ({
  ...d,
  groups: d.groups.map((g) => (g.id === id ? { ...g, label } : g)),
});
