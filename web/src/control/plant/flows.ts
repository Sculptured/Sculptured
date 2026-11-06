import type { PlantAction } from './model';

/**
 * The process view's action flows: one per agent and system it acted on, with its reads, writes,
 * unknown-effect calls and abnormal ones (refused or failed), when it last flowed, and whether that
 * was within the live window. Messages to another agent also flow agent to agent. Pure.
 */

export type Flow = { agent: string; system: string; reads: number; writes: number; other: number; abnormal: number; last: number; live: boolean };
export type AgentLink = { from: string; to: string; count: number; last: number; live: boolean };

/** How recent a flow must be to show as live. */
export const LIVE_MS = 60_000;

export function flowsOf(actions: PlantAction[], now: number, liveMs = LIVE_MS): Flow[] {
  const out = new Map<string, Flow>();
  for (const a of actions) {
    const key = `${a.agent}\u0000${a.system}`;
    const f = out.get(key) ?? { agent: a.agent, system: a.system, reads: 0, writes: 0, other: 0, abnormal: 0, last: 0, live: false };
    if (a.effect === 'read') f.reads += 1;
    else if (a.effect === 'write') f.writes += 1;
    else f.other += 1;
    if (a.outcome === 'refused' || a.outcome === 'error') f.abnormal += 1;
    f.last = Math.max(f.last, a.at);
    f.live = now - f.last <= liveMs;
    out.set(key, f);
  }
  return [...out.values()];
}

/** Messages and handoffs addressed to one agent of the team (not "everyone" or "next"). */
export function agentLinks(actions: PlantAction[], agents: string[], now: number, liveMs = LIVE_MS): AgentLink[] {
  const out = new Map<string, AgentLink>();
  for (const a of actions) {
    if (!a.to || !agents.includes(a.to) || a.to === a.agent) continue;
    const key = `${a.agent}\u0000${a.to}`;
    const l = out.get(key) ?? { from: a.agent, to: a.to, count: 0, last: 0, live: false };
    l.count += 1;
    l.last = Math.max(l.last, a.at);
    l.live = now - l.last <= liveMs;
    out.set(key, l);
  }
  return [...out.values()];
}

/** Actions per minute over the last `minutes` up to `now`, for one agent or system (a unit's key figure). */
export function ratePerMin(actions: PlantAction[], now: number, minutes = 5): number {
  const from = now - minutes * 60_000;
  return actions.filter((a) => a.at > from && a.at <= now).length / minutes;
}
