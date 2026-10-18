import type { TeamDraft } from '../generated/TeamDraft';
import { compactNumber } from '../format';
import { SOFT_SHARE, softOf } from './budgetDraft';

/** From this many agents working at once, a Start says what it may cost. */
export const MANY_AT_ONCE = 6;

type Edge = { from: string; to: string };

/** Who can work at the same moment: the agents that start together, or the most one handoff starts. */
export function mostAtOnce(d: TeamDraft, handsTo: Edge[]): { n: number; why: string } {
  const ids = new Set(d.agents.map((a) => a.id));
  const name = (id: string) => d.agents.find((a) => a.id === id)?.name.trim() || 'an agent';
  const members = (id: string) => (ids.has(id) ? [id] : d.agents.filter((a) => a.group_id === id).map((a) => a.id));
  const heads = d.agents.filter((a) => !handsTo.some((c) => c.from !== c.to && members(c.to).includes(a.id)));
  let fan = { n: 0, from: '' };
  for (const a of d.agents) {
    const n = new Set(handsTo.filter((c) => c.from === a.id).flatMap((c) => members(c.to))).size;
    if (n > fan.n) fan = { n, from: a.id };
  }
  if (fan.n >= heads.length) return { n: fan.n, why: `${name(fan.from)} hands work to ${fan.n} at once` };
  return { n: heads.length, why: `${heads.length} agents start together` };
}

const PLAN_WORDS: Record<string, string> = { five_hour: '5-hour', seven_day: 'weekly' };
const pct = (v: number) => `${+v.toFixed(1)}%`;

/** The run budget in words, and how far a stop can trail it, for the Start confirmation. */
export function budgetNotes(d: TeamDraft, atOnce: number): string[] {
  const b = d.budget;
  const parts = [
    ...(b?.plan ?? []).map((p) => `${p.provider === 'claude' ? 'Claude' : 'Codex'} ${PLAN_WORDS[p.window] ?? p.window}: wrap up at ${pct(p.soft ?? p.hard * SOFT_SHARE)}, stop at ${pct(p.hard)}`),
    ...(b?.tokens ? [`tokens: wrap up at ${compactNumber(softOf(b.tokens, true))}, stop at ${compactNumber(b.tokens.hard)}`] : []),
    ...(b?.minutes ? [`time: wrap up at ${softOf(b.minutes, true)} min, stop at ${b.minutes.hard} min`] : []),
  ];
  if (parts.length === 0) return [];
  const n = Math.max(1, Math.min(atOnce, b?.max_working ?? atOnce));
  return [
    `Run budget: ${parts.join('; ')}.`,
    `Sculptured wraps up first and stops early by about one model call for each agent working (up to ${n} at once), so a run usually ends near each hard value. Usage is reported after each call, so it can pass a hard value by a little.`,
  ];
}

/** What a busy team may cost, said before it starts; nothing for a small team with limits. */
export function efficiencyNotes(d: TeamDraft, handsTo: Edge[]): string[] {
  const { n, why } = mostAtOnce(d, handsTo);
  const budget = budgetNotes(d, n);
  if (n < MANY_AT_ONCE && d.agents.length < MANY_AT_ONCE) return budget;
  const notes: string[] = [];
  const cap = d.budget?.max_working;
  if (n >= MANY_AT_ONCE) notes.push(cap && cap < n ? `Up to ${n} agents could work at the same time (${why}); the run budget lets ${cap} work at once and the rest wait.` : `Up to ${n} agents can work at the same time: ${why}. Each uses your plan.`);
  if (!d.limits?.max_agents && !d.limits?.max_child_levels) notes.push('Agents can start helpers without limit. Set Most agents and Levels of children in Settings → More settings.');
  if (budget.length) notes.push(...budget);
  else if (!d.agents.some((a) => a.advanced.token_limit)) notes.push('No run budget is set, so nothing stops this run before your plan runs out. Set one in Settings → Run budget.');
  return notes;
}
