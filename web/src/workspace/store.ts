import type { Popped } from './layouts';
import { dropPanes, panes, WORKSPACE_LOOKS, type WorkspaceLook, type IdGen, type Layout, type Node, type PageLayouts, type PaneRef } from './model';

const KEY_PREFIX = 'sculptured.workspace.v1.';
const MAX_DEPTH = 32;

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

function validPane(v: unknown): v is PaneRef {
  return isObj(v) && typeof v.id === 'string' && typeof v.type === 'string' && (v.settings === undefined || isObj(v.settings));
}

/** A stack's own shape; its panes are checked one by one (a bad one is dropped, the rest kept). */
function validStack(v: Record<string, unknown>): boolean {
  return typeof v.id === 'string' && Array.isArray(v.panes) && Number.isInteger(v.active);
}

function validSplit(v: Record<string, unknown>, depth: number): boolean {
  if (typeof v.id !== 'string' || (v.dir !== 'row' && v.dir !== 'col')) return false;
  if (!Array.isArray(v.kids) || !Array.isArray(v.sizes) || v.kids.length === 0 || v.kids.length !== v.sizes.length) return false;
  return v.sizes.every((s) => typeof s === 'number' && Number.isFinite(s) && s > 0) && v.kids.every((k) => validNode(k, depth + 1));
}

function validNode(v: unknown, depth = 0): v is Node {
  if (!isObj(v) || depth > MAX_DEPTH) return false;
  return v.kind === 'stack' ? validStack(v) : v.kind === 'split' ? validSplit(v, depth) : false;
}

function validLayout(v: unknown): v is Layout {
  return isObj(v) && typeof v.id === 'string' && typeof v.name === 'string' && validNode(v.root) && Array.isArray(v.popped);
}

/** Drops malformed panes wherever they are; a stack's active tab stays in range. */
function onlyValidPanes(node: Node): Node {
  if (node.kind === 'split') return { ...node, kids: node.kids.map(onlyValidPanes) };
  const kept = node.panes.filter(validPane);
  return { ...node, panes: kept, active: Math.max(0, Math.min(node.active, kept.length - 1)) };
}

/** The page's readable layouts, each malformed one (or malformed pane) dropped by itself; null when none is left. */
function readPage(v: unknown): PageLayouts | null {
  if (!isObj(v) || v.version !== 1 || typeof v.active !== 'string' || !Array.isArray(v.layouts)) return null;
  const layouts = v.layouts.filter(validLayout).map((l) => ({ ...l, root: onlyValidPanes(l.root), popped: l.popped.filter(validPane) }));
  if (layouts.length === 0) return null;
  const active = layouts.some((l) => l.id === v.active) ? v.active : layouts[0].id;
  return { version: 1, active, layouts, ...(WORKSPACE_LOOKS.includes(v.look as WorkspaceLook) ? { look: v.look as WorkspaceLook } : {}) };
}

/** Keeps the first occurrence of each pane id (tree first, then popped) and drops unknown pane types. */
function cleanLayout(l: Layout, known: (type: string) => boolean, newId?: IdGen): Layout {
  const seen = new Set<string>();
  const keep = (p: PaneRef) => {
    if (!known(p.type) || seen.has(p.id)) return false;
    seen.add(p.id);
    return true;
  };
  const root = dropPanes(l.root, keep, newId);
  return { ...l, root, popped: l.popped.filter(keep) };
}

/** A layout whose panes are all unknown becomes the page default of the same id, when there is one. */
function restoreIfAllGone(saved: Layout, cleaned: Layout, defaults: () => PageLayouts): Layout {
  if (panes(saved.root).length === 0 || panes(cleaned.root).length > 0) return cleaned;
  return defaults().layouts.find((d) => d.id === saved.id) ?? cleaned;
}

/**
 * Reads the saved layouts for a page. Anything unreadable or old gives the defaults; a malformed
 * layout or pane is dropped by itself, as are unknown pane types.
 */
export function loadLayouts(page: string, defaults: () => PageLayouts, known: (type: string) => boolean, newId?: IdGen): PageLayouts {
  try {
    const value = readPage(JSON.parse(localStorage.getItem(KEY_PREFIX + page) ?? 'null'));
    if (!value) return defaults();
    const layouts = value.layouts.map((l) => restoreIfAllGone(l, cleanLayout(l, known, newId), defaults));
    return { ...value, layouts };
  } catch {
    return defaults();
  }
}

export function saveLayouts(page: string, value: PageLayouts): void {
  try { localStorage.setItem(KEY_PREFIX + page, JSON.stringify(value)); } catch { /* Still works for this session. */ }
}

const poppedKey = (page: string, channel: string) => `${KEY_PREFIX}${page}.popped.${channel}`;

/** On a page shared by many channels: which panes this channel's windows show (none when unreadable). */
export function loadPopped(page: string, channel: string): Popped {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(poppedKey(page, channel)) ?? 'null');
    if (!isObj(v)) return {};
    return Object.fromEntries(Object.entries(v).filter((e): e is [string, string[]] => Array.isArray(e[1]) && e[1].every((x) => typeof x === 'string')));
  } catch {
    return {};
  }
}

export function savePopped(page: string, channel: string, popped: Popped): void {
  try {
    if (Object.keys(popped).length === 0) localStorage.removeItem(poppedKey(page, channel));
    else localStorage.setItem(poppedKey(page, channel), JSON.stringify(popped));
  } catch { /* Still works for this session. */ }
}
