/** A block's kind; blocks from before components have none and are groups. */
export function blockKind(p: { id: string; kind?: string | null }): string {
  return p.kind ?? 'group';
}

/** Whether this version can add the block to a project or the Library (groups only, for now). */
export function opensHere(p: { id: string; kind?: string | null }): boolean {
  return blockKind(p) === 'group';
}
