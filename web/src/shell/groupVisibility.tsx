import { track } from '../hosted/analytics';
import { useCallback, useMemo, useSyncExternalStore } from 'react';
import type { CompiledEdge } from '../generated/CompiledEdge';
import { CARD_W, type Box } from '../layout';
import { routeEdges, type EdgeGeom } from './edgeGeometry';
import { Icon } from './ui';
import './group-visibility.css';

const changed = 'sculptured-group-visibility';
const fallback = new Map<string, string>();
export const groupVisibilityKey = (team: string) => `sculptured.groups.collapsed.v1.${team}`;
function read(key: string) { try { return fallback.get(key) ?? localStorage.getItem(key) ?? '[]'; } catch { return fallback.get(key) ?? '[]'; } }
function parse(raw: string): Set<string> {
  try { const a: unknown = JSON.parse(raw); return new Set(Array.isArray(a) ? a.filter((v): v is string => typeof v === 'string').slice(0, 256) : []); }
  catch { return new Set(); }
}
export function useGroupVisibility(team: string) {
  const key = groupVisibilityKey(team);
  const subscribe = useCallback((notify: () => void) => {
    window.addEventListener('storage', notify); window.addEventListener(changed, notify);
    return () => { window.removeEventListener('storage', notify); window.removeEventListener(changed, notify); };
  }, []);
  const raw = useSyncExternalStore(subscribe, () => read(key), () => '[]');
  const collapsed = useMemo(() => parse(raw), [raw]);
  const setCollapsed = useCallback((group: string, hide: boolean) => {
    const next = parse(read(key));
    if (hide) next.add(group); else next.delete(group);
    try { localStorage.setItem(key, JSON.stringify([...next].slice(-256))); fallback.delete(key); } catch { fallback.set(key, JSON.stringify([...next].slice(-256))); }
    window.dispatchEvent(new Event(changed));
  }, [key]);
  return { collapsed, setCollapsed };
}

/** Compact at the group's centre. Saved coordinates and neighbouring nodes never move. */
export function closedGroupBox(b: Box): Box {
  const w = Math.min(b.w, CARD_W), h = Math.min(b.h, 132);
  return { x: b.x + (b.w - w) / 2, y: b.y + (b.h - h) / 2, w, h };
}
export function projectGroups(boxes: Record<string, Box>, membership: Record<string, string>, groups: Record<string, Box>, collapsed: ReadonlySet<string>) {
  const hidden = new Map<string, string>();
  const visible = { ...boxes };
  for (const [id, group] of Object.entries(membership)) {
    if (!collapsed.has(group) || !groups[group]) continue;
    hidden.set(id, group); delete visible[id];
  }
  for (const group of collapsed) if (groups[group]) visible[`\0group:${group}`] = closedGroupBox(groups[group]);
  return { boxes: visible, hidden };
}
export type GroupProjection = ReturnType<typeof projectGroups>;
/** A routed edge. Arrows that touch a collapsed group and share both drawn ends are drawn once, as the
 *  first edge of the bundle (a handoff before a softer link); `merged` lists every edge it stands for. */
export type GroupEdgeGeom = EdgeGeom & { merged: CompiledEdge[] };
/** Keep original edge keys/provenance while routing boundary edges to the group box. */
export function routeGroupEdges(edges: CompiledEdge[], projection: GroupProjection): Map<CompiledEdge, GroupEdgeGeom> {
  const bundles = new Map<string, { projected: CompiledEdge; merged: CompiledEdge[] }>();
  edges.forEach((original, i) => {
    const from = projection.hidden.get(original.from), to = projection.hidden.get(original.to);
    if (from !== undefined && from === to) return;
    const projected = { ...original, from: from === undefined ? original.from : `\0group:${from}`, to: to === undefined ? original.to : `\0group:${to}` };
    const key = from === undefined && to === undefined ? `edge:${i}` : `${projected.from}\n${projected.to}`;
    const b = bundles.get(key);
    if (!b) bundles.set(key, { projected, merged: [original] });
    else if (original.enforced && !b.merged[0].enforced) { b.merged.unshift(original); b.projected = projected; }
    else b.merged.push(original);
  });
  const list = [...bundles.values()];
  const routes = routeEdges(list.map(b => b.projected), projection.boxes);
  return new Map(list.flatMap(b => { const r = routes.get(b.projected); return r ? [[b.merged[0], { ...r, merged: b.merged }] as const] : []; }));
}
export function GroupVisibilityButton({ name, collapsed, onToggle, onFocus }: { name: string; collapsed: boolean; onToggle: () => void; onFocus?: () => void }) {
  return <button type="button" className="group-visibility" aria-label={`${collapsed ? 'Show' : 'Hide'} contents of ${name}`} aria-expanded={!collapsed}
    onFocus={onFocus} onPointerDown={e => e.stopPropagation()} onClick={e => { e.stopPropagation(); if (!collapsed) track('collapse-group'); onToggle(); }}>
    <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={13} /><span className="gv-word">{collapsed ? 'Show contents' : 'Hide contents'}</span>
  </button>;
}
