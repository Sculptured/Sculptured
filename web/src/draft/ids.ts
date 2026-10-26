import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import type { NameMap } from '../vocab';

/** Agents without a group compile into this group (pod). Same as the server's DEFAULT_GROUP_ID. */
export const DEFAULT_GROUP_ID = 'team';

/** The group an agent belongs to: its own, or the main group when it has none. */
export const groupOf = (a: { group_id: string | null }): string => a.group_id ?? DEFAULT_GROUP_ID;

/** Lowercase letters and digits joined by '-', at most 48 characters; `fallback` if empty. */
export function slug(name: string, fallback: string): string {
  let out = '';
  let dash = false;
  for (const ch of name.trim()) {
    if (/^[A-Za-z0-9]$/.test(ch)) {
      out += ch.toLowerCase();
      dash = false;
    } else if (out && !dash) {
      out += '-';
      dash = true;
    }
  }
  out = out.slice(0, 48).replace(/-+$/, '');
  return out || fallback;
}

const ALPHABET = 'abcdefghijklmnopqrstuvwxyz0123456789';

/** Four random characters from [a-z0-9]. */
export function randomSuffix(): string {
  const bytes = new Uint8Array(4);
  globalThis.crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => ALPHABET[b % ALPHABET.length]).join('');
}

/** `${base}-xxxx`, not in `taken`. Assigned once; never derived from a later name. */
export function newId(base: string, taken: Set<string>): string {
  for (;;) {
    const id = `${base}-${randomSuffix()}`;
    if (!taken.has(id)) return id;
  }
}

export const KIND_BASE: Record<AgentKind, string> = { claude: 'Claude', codex: 'Codex', opencode: 'OpenCode', practice: 'Practice' };

export function nextAgentName(d: TeamDraft, kind: AgentKind): string {
  const used = new Set(d.agents.map((a) => a.name.trim().toLowerCase()));
  for (let n = 1; ; n++) {
    const name = `${KIND_BASE[kind]} ${n}`;
    if (!used.has(name.toLowerCase())) return name;
  }
}

/** `group.agent@team`, from ids only (matches the server's TeamDraft::seat_id). */
export function seatIdFor(d: TeamDraft, agentId: string): string | null {
  const a = d.agents.find((x) => x.id === agentId);
  return a ? `${a.group_id ?? DEFAULT_GROUP_ID}.${a.id}@${d.id}` : null;
}

/** Names to show in Run mode, keyed by seat id. */
export function nameMap(d: TeamDraft): NameMap {
  return Object.fromEntries(d.agents.map((a) => [seatIdFor(d, a.id)!, a.name.trim() || 'Unnamed agent']));
}
