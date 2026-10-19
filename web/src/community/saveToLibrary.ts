import { api } from '../api';
import type { GroupBlock } from '../generated/GroupBlock';
import type { Publication } from '../generated/community/Publication';
import { sanitizeExport } from '../release/sharing';

/** Whether a Community block is in the Library: not at all, this version, or an older one. */
export type LibraryState = { kind: 'none' } | { kind: 'same'; copy: GroupBlock } | { kind: 'older'; copy: GroupBlock; has: number };

export function libraryState(pub: Publication, library: GroupBlock[]): LibraryState {
  const copy = library.find((b) => b.source?.community_block === pub.id);
  if (!copy?.source) return { kind: 'none' };
  return copy.source.version >= pub.version ? { kind: 'same', copy } : { kind: 'older', copy, has: copy.source.version };
}

/** The Library copy of a block, replacing `existing` (same id and revision) when updating, so there is never a duplicate. */
export function libraryCopy(pub: Publication, existing?: GroupBlock): GroupBlock {
  const b = sanitizeExport(pub.content);
  return { ...b, id: existing?.id ?? `community-${pub.id}`, revision: existing?.revision ?? 0, name: pub.title, source: { community_block: pub.id, version: pub.version } };
}

/** Saves (or updates) the block in the Library and returns the saved copy. */
export async function saveToLibrary(pub: Publication): Promise<GroupBlock> {
  const state = libraryState(pub, await api.groupBlocks());
  return api.saveGroupBlock(libraryCopy(pub, state.kind === 'none' ? undefined : state.copy));
}
