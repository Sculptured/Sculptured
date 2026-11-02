import { dropPanes, panes, type IdGen, type Layout, type Node, type PageLayouts, type PaneRef } from './model';

const KEY_PREFIX = 'sculptured.workspace.v1.';
const MAX_DEPTH = 32;

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

function validPane(v: unknown): v is PaneRef {
  return isObj(v) && typeof v.id === 'string' && typeof v.type === 'string' && (v.settings === undefined || isObj(v.settings));
}

function validStack(v: Record<string, unknown>): boolean {
  return typeof v.id === 'string' && Array.isArray(v.panes) && v.panes.every(validPane) && Number.isInteger(v.active);
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
  return isObj(v) && typeof v.id === 'string' && typeof v.name === 'string' && validNode(v.root) && Array.isArray(v.popped) && v.popped.every(validPane);
}

function validPage(v: unknown): v is PageLayouts {
  if (!isObj(v) || v.version !== 1 || typeof v.active !== 'string') return false;
  if (!Array.isArray(v.layouts) || v.layouts.length === 0 || !v.layouts.every(validLayout)) return false;
  return v.layouts.some((l) => l.id === v.active);
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

/** Reads the saved layouts for a page. Anything unreadable, old or malformed gives the defaults; unknown pane types are dropped. */
export function loadLayouts(page: string, defaults: () => PageLayouts, known: (type: string) => boolean, newId?: IdGen): PageLayouts {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY_PREFIX + page) ?? 'null');
    if (!validPage(value)) return defaults();
    const layouts = value.layouts.map((l) => restoreIfAllGone(l, cleanLayout(l, known, newId), defaults));
    return { ...value, layouts };
  } catch {
    return defaults();
  }
}

export function saveLayouts(page: string, value: PageLayouts): void {
  try { localStorage.setItem(KEY_PREFIX + page, JSON.stringify(value)); } catch { /* Still works for this session. */ }
}
