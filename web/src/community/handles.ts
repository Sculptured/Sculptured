import { COMMUNITY_ERROR_WORDS } from '../errors';

/** Community's handle rules: 3–30 of a–z, 0–9 and dashes, starting and ending with a letter or digit. */
export const HANDLE_RE = /^[a-z0-9](?:[a-z0-9-]{1,28}[a-z0-9])$/;
export const RESERVED_HANDLES: readonly string[] = ['admin', 'administrator', 'sculptured', 'official', 'support', 'help', 'root', 'system', 'moderator', 'community', 'api', 'about', 'settings', 'me', 'people', 'u'];

/** Why a handle would be refused, in words, or null when it is fine. */
export function handleProblem(h: string): string | null {
  if (RESERVED_HANDLES.includes(h)) return COMMUNITY_ERROR_WORDS.handle_reserved;
  if (!HANDLE_RE.test(h)) return COMMUNITY_ERROR_WORDS.handle_invalid;
  return null;
}

/** A handle made from a name that passes the rules (`Ana Lee!` → `ana-lee`). */
export function suggestHandle(name: string): string {
  let base = name.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 24).replace(/-+$/, '');
  if (base.length < 3) base = 'member';
  return RESERVED_HANDLES.includes(base) ? `${base}-1` : base;
}
