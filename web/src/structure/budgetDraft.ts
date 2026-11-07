import type { PlanBudget } from '../generated/PlanBudget';
import type { PlanPause } from '../generated/PlanPause';
import type { RunBudget } from '../generated/RunBudget';
import type { SoftHard } from '../generated/SoftHard';
import type { TeamDraft } from '../generated/TeamDraft';

/** The plan windows a run budget can name, in the order the form shows them. */
export const PLAN_ROWS: { provider: 'claude' | 'codex'; window: 'five_hour' | 'seven_day'; label: string }[] = [
  { provider: 'claude', window: 'five_hour', label: 'Claude, 5-hour window' },
  { provider: 'claude', window: 'seven_day', label: 'Claude, weekly' },
  { provider: 'codex', window: 'five_hour', label: 'Codex, 5-hour window' },
  { provider: 'codex', window: 'seven_day', label: 'Codex, weekly' },
];

/** The soft value a limit gets when only its hard value is set (the server applies the same). */
export const SOFT_SHARE = 0.85;
export const softOf = (v: SoftHard<number>, whole = false) => v.soft ?? (whole ? Math.max(Math.floor(v.hard * SOFT_SHARE), Math.min(v.hard, 1)) : v.hard * SOFT_SHARE);

const isEmpty = (b: RunBudget) => !b.plan?.length && !b.tokens && !b.minutes && b.max_working == null;

/** The draft with its budget changed by `f`; an empty budget drops `budget` altogether. */
export function withBudget(d: TeamDraft, f: (b: RunBudget) => RunBudget): TeamDraft {
  const next = f({ ...(d.budget ?? {}) });
  const { budget: _old, ...rest } = d;
  return isEmpty(next) ? rest : { ...rest, budget: next };
}

/** A soft/hard pair from two typed values: no hard value means no limit; soft is optional. */
export function softHard(soft: number | null, hard: number | null): SoftHard<number> | undefined {
  if (hard === null || hard <= 0) return undefined;
  return soft === null ? { hard } : { soft, hard };
}

/** The budget with one plan window's limit set, or removed. */
export function withPlan(b: RunBudget, provider: string, window: string, v: SoftHard<number> | undefined): RunBudget {
  const was = planOf(b, provider, window);
  const others = (b.plan ?? []).filter((p) => !(p.provider === provider && p.window === window));
  // Changing the values keeps what happens at the hard value.
  const plan: PlanBudget[] = v ? [...others, { provider, window, ...v, ...(was?.pause ? { pause: was.pause } : {}) }] : others;
  const { plan: _p, ...rest } = b;
  return plan.length ? { ...rest, plan } : rest;
}

/** The budget with one plan window pausing at its hard value (`pause`), or stopping (undefined). */
export function withPause(b: RunBudget, provider: string, window: string, pause: PlanPause | undefined): RunBudget {
  const plan = (b.plan ?? []).map((p) => {
    if (p.provider !== provider || p.window !== window) return p;
    const { pause: _old, ...rest } = p;
    return pause ? { ...rest, pause } : rest;
  });
  return { ...b, plan };
}

export const planOf = (b: RunBudget | undefined, provider: string, window: string) => b?.plan?.find((p) => p.provider === provider && p.window === window);

/** Problems the server would refuse, in the same words, so the form can say them first. */
export function budgetProblems(b: RunBudget | undefined): string[] {
  if (!b) return [];
  const out = (b.plan ?? []).flatMap(planProblems);
  if (b.tokens?.soft != null && b.tokens.soft > b.tokens.hard) out.push('Token budget: the soft value is above the hard value');
  if (b.minutes?.soft != null && b.minutes.soft > b.minutes.hard) out.push('Time budget: the soft value is above the hard value');
  if (b.max_working === 0) out.push('Most agents working at once must be at least 1');
  return out;
}

function planProblems(p: PlanBudget): string[] {
  const out: string[] = [];
  if (p.hard <= 0 || p.hard > 100) out.push(`Plan budget: ${p.hard} is not a percent between 0 and 100`);
  else if (p.soft != null && p.soft > p.hard) out.push('Plan budget: the soft value is above the hard value');
  if (p.pause && p.pause.max_windows < 1) out.push('Plan budget: a paused run needs at least 1 window');
  return out;
}
