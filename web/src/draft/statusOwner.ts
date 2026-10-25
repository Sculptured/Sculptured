import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';

/** `id` writes the team's status page (null: Sculptured chooses); at most one agent is chosen. */
export const setStatusOwner = (d: TeamDraft, id: string | null): TeamDraft => ({
  ...d,
  agents: d.agents.map((a) => {
    const { writes_status: _, ...rest } = a.advanced;
    return { ...a, advanced: a.id === id ? { ...rest, writes_status: true } : rest };
  }),
});

/**
 * The agent that writes the team's status page, as the server decides it (`TeamDraft::status_owner`):
 * the one chosen; else the workflow's collector; else the one agent at the end of the line; else the
 * one that leads; else the first agent.
 */
export function statusOwner(d: TeamDraft): DraftAgent | null {
  const chosen = d.agents.find((a) => a.advanced.writes_status);
  if (chosen) return chosen;
  const collector = d.workflow && d.agents.find((a) => a.id === d.workflow!.collector);
  if (collector) return collector;
  const hands = (a: DraftAgent, from: boolean) => d.connections.some((c) => c.kind === 'hands-work-to' && (from ? c.from : c.to) === a.id);
  const only = (keep: (a: DraftAgent) => boolean) => { const m = d.agents.filter(keep); return m.length === 1 ? m[0] : null; };
  return only((a) => hands(a, false) && !hands(a, true)) ?? only((a) => hands(a, true) && !hands(a, false)) ?? d.agents[0] ?? null;
}
