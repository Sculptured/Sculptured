import type { Component } from '../generated/Component';
/** A block's kind; blocks from before components have none and are groups. */
export function blockKind(p: { id: string; kind?: string | null }): string {
  return p.kind ?? 'group';
}

/** Whether this version can add the block to a project or the Library (groups only, for now). */
export function opensHere(p: { id: string; kind?: string | null }): boolean {
  return blockKind(p) === 'group';
}

/** A component block's manifest (its content is `{ kind: 'component', component }`); null for a group. */
export function componentOf(p: { kind?: string | null; content: unknown }): Component | null {
  if (blockKind({ id: '', kind: p.kind }) !== 'component') return null;
  return (p.content as { component?: Component } | null)?.component ?? null;
}
