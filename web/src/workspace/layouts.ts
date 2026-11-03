/** Pure operations on a page's layouts (the layout tabs) and on popped-out panes. */
import { addPane, dropPanes, freshId, panes, type IdGen, type Layout, type Node, type PageLayouts, type PaneRef } from './model';
import { isSlot, slotFor, slotRef } from './slots';

const findPopped = (l: Layout, instance: string | undefined) => l.popped.find((p) => p.id === instance);

function cloneNode(node: Node, swap: (p: PaneRef) => PaneRef, newId: IdGen): Node {
  if (node.kind === 'split') return { ...node, id: newId(), kids: node.kids.map((k) => cloneNode(k, swap, newId)) };
  return { ...node, id: newId(), panes: node.panes.map(swap) };
}

/** A copy with fresh ids throughout. Popped-out panes are back in their slots in the copy; it has none popped. */
export function cloneLayout(l: Layout, patch: Pick<Layout, 'id' | 'name'>, newId: IdGen = freshId): Layout {
  const kept = dropPanes(l.root, (p) => !isSlot(p) || !!findPopped(l, slotFor(p)), newId);
  const swap = (p: PaneRef): PaneRef => {
    const real = isSlot(p) ? findPopped(l, slotFor(p))! : p;
    return { ...real, id: newId(), ...(real.settings && { settings: structuredClone(real.settings) }) };
  };
  return { ...patch, root: cloneNode(kept, swap, newId), popped: [] };
}

export function uniqueName(layouts: Layout[], base: string): string {
  const taken = new Set(layouts.map((l) => l.name));
  if (!taken.has(base)) return base;
  let n = 2;
  while (taken.has(`${base} ${n}`)) n++;
  return `${base} ${n}`;
}

function insertAfter(v: PageLayouts, afterId: string, l: Layout): PageLayouts {
  const at = v.layouts.findIndex((x) => x.id === afterId);
  const layouts = [...v.layouts];
  layouts.splice(at < 0 ? layouts.length : at + 1, 0, l);
  return { ...v, layouts, active: l.id };
}

/** "+": a copy of the page's first default layout, made active. */
export function addLayout(v: PageLayouts, defaults: PageLayouts, newId: IdGen = freshId): PageLayouts {
  const first = defaults.layouts[0];
  if (!first) return v;
  const l = cloneLayout(first, { id: newId(), name: uniqueName(v.layouts, first.name) }, newId);
  return insertAfter(v, v.layouts[v.layouts.length - 1]?.id ?? '', l);
}

export function duplicateLayout(v: PageLayouts, id: string, newId: IdGen = freshId): PageLayouts {
  const l = v.layouts.find((x) => x.id === id);
  if (!l) return v;
  return insertAfter(v, id, cloneLayout(l, { id: newId(), name: uniqueName(v.layouts, `${l.name} copy`) }, newId));
}

/** Never removes the last layout. Deleting the active one activates its neighbour. */
export function deleteLayout(v: PageLayouts, id: string): PageLayouts {
  const at = v.layouts.findIndex((x) => x.id === id);
  if (at < 0 || v.layouts.length < 2) return v;
  const layouts = v.layouts.filter((x) => x.id !== id);
  const active = v.active === id ? layouts[Math.min(at, layouts.length - 1)].id : v.active;
  return { ...v, layouts, active };
}

export function renameLayout(v: PageLayouts, id: string, name: string): PageLayouts {
  const clean = name.trim();
  if (!clean) return v;
  return { ...v, layouts: v.layouts.map((l) => (l.id === id ? { ...l, name: clean } : l)) };
}

/** Back to the default of the same id (name included), else to the first default under its own name. */
export function resetLayout(v: PageLayouts, id: string, defaults: PageLayouts, newId: IdGen = freshId): PageLayouts {
  const l = v.layouts.find((x) => x.id === id);
  const match = defaults.layouts.find((d) => d.id === id);
  const from = match ?? defaults.layouts[0];
  if (!l || !from) return v;
  const fresh = cloneLayout(from, { id, name: match ? match.name : l.name }, newId);
  return { ...v, layouts: v.layouts.map((x) => (x.id === id ? fresh : x)) };
}

function mapPanes(node: Node, fn: (p: PaneRef) => PaneRef): Node {
  if (node.kind === 'split') return { ...node, kids: node.kids.map((k) => mapPanes(k, fn)) };
  return { ...node, panes: node.panes.map(fn) };
}

/** Moves a pane to `popped`, leaving a placeholder in its slot. */
export function popOutPane(l: Layout, paneId: string, newId: IdGen = freshId): Layout {
  const pane = panes(l.root).find((p) => p.id === paneId);
  if (!pane || isSlot(pane)) return l;
  const slot = slotRef(newId(), pane);
  return { ...l, root: mapPanes(l.root, (p) => (p.id === paneId ? slot : p)), popped: [...l.popped, pane] };
}

/** Puts a popped pane back in its slot, or in `stackId` (else the first stack) if its slot was closed. */
export function bringBack(l: Layout, instance: string, stackId?: string): Layout {
  const pane = findPopped(l, instance);
  if (!pane) return l;
  const popped = l.popped.filter((p) => p !== pane);
  const slot = panes(l.root).find((p) => slotFor(p) === instance);
  const root = slot ? mapPanes(l.root, (p) => (p.id === slot.id ? pane : p)) : addPane(l.root, pane, stackId);
  return { ...l, root, popped };
}

export function setPoppedSettings(l: Layout, instance: string, settings: Record<string, unknown>): Layout {
  if (!findPopped(l, instance)) return l;
  return { ...l, popped: l.popped.map((p) => (p.id === instance ? { ...p, settings } : p)) };
}

/** Runs `fn` on every layout, keeping the value unchanged when no layout changed. */
export function eachLayout(v: PageLayouts, fn: (l: Layout) => Layout): PageLayouts {
  const layouts = v.layouts.map(fn);
  return layouts.every((l, i) => l === v.layouts[i]) ? v : { ...v, layouts };
}
