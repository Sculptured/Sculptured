import { plural } from '../format';

/**
 * The Input is plain text, saved as written. These helpers read it as a list of things a person
 * added (links, notes, files) and add or remove one without touching the rest.
 *
 * - A line with a web address is a link.
 * - A line that is a path (`/…`, `~/…`, `C:\…`) is a file (Upload adds each file on its own line).
 * - A bulleted line (`- `, `* `, `• `, `1. `) is a note of its own.
 * - Other lines run together into one note until a blank line.
 */
export type InputItemKind = 'link' | 'note' | 'file';
export type InputItem = {
  kind: InputItemKind;
  /** The item's text as written (lines joined with a newline). */
  text: string;
  /** Link: the web address. File: the path. */
  target?: string;
  /** Lines `[start, end)` of the Input's text that hold it. */
  start: number;
  end: number;
};

const URL_IN_LINE = /\bhttps?:\/\/[^\s<>()"']+/i;
const PATH_LINE = /^(?:\/|~\/|[A-Za-z]:[\\/])\S/;
const BULLET = /^(?:[-*•]|\d+[.)])\s+/;

/** A line that is an item on its own (a link, a file, a bulleted note), else null. */
function lineItem(line: string, i: number): InputItem | null {
  const url = line.match(URL_IN_LINE)?.[0];
  if (url) return { kind: 'link', text: line, target: url.replace(/[.,;:!?]+$/, ''), start: i, end: i + 1 };
  if (PATH_LINE.test(line)) return { kind: 'file', text: line, target: line, start: i, end: i + 1 };
  if (BULLET.test(line)) return { kind: 'note', text: line.replace(BULLET, ''), start: i, end: i + 1 };
  return null;
}

export function inputItems(text: string): InputItem[] {
  const items: InputItem[] = [];
  let note: InputItem | null = null;
  const close = () => {
    if (note) items.push(note);
    note = null;
  };
  text.split('\n').forEach((raw, i) => {
    const line = raw.trim();
    if (!line) return close();
    const own = lineItem(line, i);
    if (own) {
      close();
      items.push(own);
    } else if (note) {
      const n: InputItem = note;
      n.text += `\n${line}`;
      n.end = i + 1;
    } else note = { kind: 'note', text: line, start: i, end: i + 1 };
  });
  close();
  return items;
}

export type InputCounts = Record<InputItemKind, number>;

export function inputCounts(items: InputItem[]): InputCounts {
  const c: InputCounts = { link: 0, note: 0, file: 0 };
  for (const it of items) c[it.kind]++;
  return c;
}

/** "2 links · 1 note · 3 files", leaving out what is not there; "" when nothing is. */
export function countsText(c: InputCounts): string {
  return ([['link', 'link'], ['note', 'note'], ['file', 'file']] as const)
    .filter(([k]) => c[k] > 0)
    .map(([k, word]) => plural(c[k], word))
    .join(' · ');
}

/** Lines joined back, with at most one blank line in a row and none at the ends. */
function tidy(lines: string[]): string {
  return lines.join('\n').replace(/\n{3,}/g, '\n\n').replace(/^\n+|\n+$/g, '');
}

/** The text without one item; every other line stays as written. */
export function removeInputItem(text: string, item: Pick<InputItem, 'start' | 'end'>): string {
  const lines = text.split('\n');
  lines.splice(item.start, item.end - item.start);
  return tidy(lines);
}

/** Adds a web address on its own line. */
export function addInputLink(text: string, url: string): string {
  const base = text.trimEnd();
  return base ? `${base}\n${url.trim()}` : url.trim();
}

/** Adds a note as its own paragraph. */
export function addInputNote(text: string, note: string): string {
  const base = text.trimEnd();
  const body = note.trim();
  return base ? `${base}\n\n${body}` : body;
}

/**
 * What was typed as a link, as a web address: `example.com/x` becomes `https://example.com/x`.
 * Null when it is not a web address, with nothing guessed.
 */
export function normalizeLink(typed: string): string | null {
  const t = typed.trim();
  if (!t || /\s/.test(t)) return null;
  const withScheme = /^https?:\/\//i.test(t) ? t : /^[\w-]+(\.[\w-]+)+(?:[/:?#]|$)/.test(t) ? `https://${t}` : null;
  if (!withScheme) return null;
  try {
    const u = new URL(withScheme);
    return u.hostname.includes('.') || u.hostname === 'localhost' ? withScheme : null;
  } catch {
    return null;
  }
}

/** A link shown short: its host and path, without the scheme or a trailing slash. */
export function shortLink(url: string): string {
  return url.replace(/^https?:\/\//i, '').replace(/^www\./i, '').replace(/\/$/, '');
}

/** A file shown short: its name. */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
