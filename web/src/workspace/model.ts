/** Pure layout tree for the dockable workspace. Every function returns a new tree and never mutates its input. */
export type PaneRef = { id: string; type: string; settings?: Record<string, unknown> };
export type Stack = { kind: 'stack'; id: string; panes: PaneRef[]; active: number };
export type Split = { kind: 'split'; id: string; dir: 'row' | 'col'; kids: Node[]; sizes: number[] };
export type Node = Stack | Split;
export type Zone = 'left' | 'right' | 'top' | 'bottom' | 'center';
export type Layout = { id: string; name: string; root: Node; popped: PaneRef[] };
export type PageLayouts = { version: 1; active: string; layouts: Layout[] };
/** Makes ids for new stacks and splits; tests pass a counter. */
export type IdGen = () => string;

export const MIN_SIZE = 0.15;
const uid: IdGen = () => crypto.randomUUID();

export function emptyStack(newId: IdGen = uid): Stack {
  return { kind: 'stack', id: newId(), panes: [], active: 0 };
}

export function stacks(root: Node): Stack[] {
  return root.kind === 'stack' ? [root] : root.kids.flatMap(stacks);
}

export function panes(root: Node): PaneRef[] {
  return stacks(root).flatMap((s) => s.panes);
}

function clampActive(panes: PaneRef[], active: number): number {
  return Math.max(0, Math.min(active, panes.length - 1));
}

function mapStacks(node: Node, fn: (s: Stack) => Stack): Node {
  if (node.kind === 'stack') return fn(node);
  return { ...node, kids: node.kids.map((k) => mapStacks(k, fn)) };
}

function normalizeSizes(sizes: number[]): number[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  return total > 0 ? sizes.map((s) => s / total) : sizes.map(() => 1 / sizes.length);
}

/** Drops empty stacks and one-child splits, and merges a split into a parent split of the same direction. */
function prune(node: Node): Node | null {
  if (node.kind === 'stack') return node.panes.length ? node : null;
  const kids: Node[] = [];
  const sizes: number[] = [];
  node.kids.forEach((kid, i) => {
    const next = prune(kid);
    if (!next) return;
    const share = node.sizes[i] ?? 0;
    if (next.kind === 'split' && next.dir === node.dir) {
      kids.push(...next.kids);
      sizes.push(...next.sizes.map((s) => s * share));
    } else {
      kids.push(next);
      sizes.push(share);
    }
  });
  if (kids.length === 0) return null;
  if (kids.length === 1) return kids[0];
  return { ...node, kids, sizes: normalizeSizes(sizes) };
}

/** An empty tree becomes a single empty stack. */
export function tidy(root: Node, newId: IdGen = uid): Node {
  if (root.kind === 'stack') return root;
  return prune(root) ?? emptyStack(newId);
}

function removePane(root: Node, paneId: string): Node {
  return mapStacks(root, (s) => {
    const at = s.panes.findIndex((p) => p.id === paneId);
    if (at < 0) return s;
    const kept = s.panes.filter((p) => p.id !== paneId);
    const active = at < s.active ? s.active - 1 : s.active;
    return { ...s, panes: kept, active: clampActive(kept, active) };
  });
}

function findPane(root: Node, paneId: string): { pane: PaneRef; stack: Stack } | undefined {
  for (const stack of stacks(root)) {
    const pane = stack.panes.find((p) => p.id === paneId);
    if (pane) return { pane, stack };
  }
  return undefined;
}

function addToStack(root: Node, stackId: string, pane: PaneRef): Node {
  return mapStacks(root, (s) => (s.id === stackId ? { ...s, panes: [...s.panes, pane], active: s.panes.length } : s));
}

const DIR = { left: 'row', right: 'row', top: 'col', bottom: 'col' } as const;

/** Puts `fresh` beside the stack `targetId`, splitting that stack's share of the space in half. */
function insertBeside(node: Node, targetId: string, fresh: Stack, zone: Exclude<Zone, 'center'>, newId: IdGen): Node {
  const dir = DIR[zone];
  const before = zone === 'left' || zone === 'top';
  if (node.kind === 'stack') {
    if (node.id !== targetId) return node;
    return { kind: 'split', id: newId(), dir, kids: before ? [fresh, node] : [node, fresh], sizes: [0.5, 0.5] };
  }
  const at = node.kids.findIndex((k) => k.kind === 'stack' && k.id === targetId);
  if (at >= 0 && node.dir === dir) {
    const kids = [...node.kids];
    const sizes = [...node.sizes];
    const half = sizes[at] / 2;
    kids.splice(before ? at : at + 1, 0, fresh);
    sizes.splice(at, 1, half, half);
    return { ...node, kids, sizes };
  }
  return { ...node, kids: node.kids.map((k) => insertBeside(k, targetId, fresh, zone, newId)) };
}

export function movePane(root: Node, paneId: string, targetStackId: string, zone: Zone, newId: IdGen = uid): Node {
  const found = findPane(root, paneId);
  const target = stacks(root).find((s) => s.id === targetStackId);
  if (!found || !target) return root;
  const sameStack = found.stack.id === target.id;
  if (sameStack && (zone === 'center' || target.panes.length === 1)) return root;
  const without = removePane(root, paneId);
  if (zone === 'center') return tidy(addToStack(without, target.id, found.pane), newId);
  const fresh: Stack = { kind: 'stack', id: newId(), panes: [found.pane], active: 0 };
  return tidy(insertBeside(without, target.id, fresh, zone, newId), newId);
}

export function closePane(root: Node, paneId: string, newId: IdGen = uid): Node {
  if (!findPane(root, paneId)) return root;
  return tidy(removePane(root, paneId), newId);
}

export function addPane(root: Node, pane: PaneRef, targetStackId?: string): Node {
  const all = stacks(root);
  const target = all.find((s) => s.id === targetStackId) ?? all[0];
  return addToStack(root, target.id, pane);
}

export function duplicatePane(root: Node, paneId: string, newId: string): Node {
  const found = findPane(root, paneId);
  if (!found) return root;
  const copy: PaneRef = { ...found.pane, id: newId };
  if (found.pane.settings) copy.settings = structuredClone(found.pane.settings);
  return mapStacks(root, (s) => {
    if (s.id !== found.stack.id) return s;
    const at = s.panes.findIndex((p) => p.id === paneId);
    const next = [...s.panes.slice(0, at + 1), copy, ...s.panes.slice(at + 1)];
    return { ...s, panes: next, active: at + 1 };
  });
}

/** Moves the divider after kid `index` by `delta` (fractions of the split), keeping both neighbours at least MIN_SIZE. */
export function resize(root: Node, splitId: string, index: number, delta: number): Node {
  if (root.kind === 'stack') return root;
  if (root.id !== splitId) return { ...root, kids: root.kids.map((k) => resize(k, splitId, index, delta)) };
  const a = root.sizes[index];
  const b = root.sizes[index + 1];
  if (a === undefined || b === undefined) return root;
  const lo = MIN_SIZE - a;
  const hi = b - MIN_SIZE;
  if (lo > hi) return root;
  const d = Math.max(lo, Math.min(hi, delta));
  const sizes = [...root.sizes];
  sizes[index] = a + d;
  sizes[index + 1] = b - d;
  return { ...root, sizes };
}

export function setActive(root: Node, stackId: string, index: number): Node {
  return mapStacks(root, (s) => (s.id === stackId && s.panes.length ? { ...s, active: clampActive(s.panes, index) } : s));
}

/** Keeps only the panes `keep` accepts (visited in tree order), clamps every stack's active tab into range, and tidies. */
export function dropPanes(root: Node, keep: (pane: PaneRef) => boolean, newId: IdGen = uid): Node {
  const filtered = mapStacks(root, (s) => {
    const kept = s.panes.filter(keep);
    return { ...s, panes: kept, active: clampActive(kept, s.active) };
  });
  return tidy(filtered, newId);
}

export function dropUnknown(root: Node, known: (type: string) => boolean, newId: IdGen = uid): Node {
  return dropPanes(root, (p) => known(p.type), newId);
}
