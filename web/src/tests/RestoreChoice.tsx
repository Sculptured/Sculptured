import { useEffect, useRef, useState } from 'react';
import { ApiError } from '../api';
import { Segmented } from '../shell/ui';
import { REPLAY_NOTE, planLine } from './restore';
import { ErrorNote } from './shared';
import type { PartPlan, RestoreMode, RestorePlan } from './types';

/** What the restore plan for the panel's step says: loading, the parts, a refusal (409) or a failure. */
export type Plan =
  | { state: 'loading' }
  | { state: 'ready'; parts: PartPlan[] }
  | { state: 'refused'; reason: string }
  | { state: 'failed'; error: unknown };

const MODES: { value: RestoreMode; label: string; tip: string }[] = [
  { value: 'exact', label: 'Exact', tip: 'The world exactly as it was before this step' },
  { value: 'approximate', label: 'Approximate', tip: 'From saves that may differ slightly; possible when exact is not' },
];

/** Reads a restore plan for a step and a restore mode (the source's `timeline.plan`). */
export type PlanLoader = (seq: number, mode: RestoreMode) => Promise<RestorePlan>;

/** The plan for a fork at `seq` restored in `mode`; a refusal keeps the engine's own words. */
async function loadPlan(load: PlanLoader, seq: number, mode: RestoreMode): Promise<Plan> {
  try {
    return { state: 'ready', parts: (await load(seq, mode)).parts };
  } catch (e) {
    if (e instanceof ApiError && e.status === 409) return { state: 'refused', reason: e.raw ?? e.message };
    return { state: 'failed', error: e };
  }
}

/** The plan for the current step and mode, read again when either changes. */
export function useRestorePlan(load: PlanLoader, seq: number, mode: RestoreMode): Plan {
  const [plan, setPlan] = useState<Plan>({ state: 'loading' });
  const latest = useRef(load);
  useEffect(() => { latest.current = load; });
  useEffect(() => {
    let alive = true;
    setPlan({ state: 'loading' });
    void loadPlan(latest.current, seq, mode).then((p) => { if (alive) setPlan(p); });
    return () => { alive = false; };
  }, [seq, mode]);
  return plan;
}

export const WAIT_REASON = 'Wait for this branch to finish first.';

/** Why the fork can't start: the branch is running, or the restore is refused; null when it can. */
export function blockedReason(plan: Plan, running: boolean): string | null {
  if (running) return WAIT_REASON;
  return plan.state === 'refused' ? plan.reason : null;
}

function PlanLines({ plan }: { plan: Plan }) {
  if (plan.state === 'loading') return <p className="muted tst-small">Checking how each part is restored…</p>;
  // A refusal is said next to the start button it disables.
  if (plan.state === 'refused') return null;
  if (plan.state === 'failed') return <ErrorNote error={plan.error} />;
  return (
    <>
      <ul className="tst-plan">{plan.parts.map((p) => <li key={p.part} className="mono tst-small">{planLine(p)}</li>)}</ul>
      {plan.parts.some((p) => p.how === 'replay') && <p className="muted tst-small">{REPLAY_NOTE}</p>}
    </>
  );
}

/** The "Restore" choice (Exact or Approximate) and, per part, how it would be restored. */
export function RestoreChoice({ mode, onMode, plan }: { mode: RestoreMode; onMode: (m: RestoreMode) => void; plan: Plan }) {
  return (
    <div className="tst-restore">
      <div className="tst-restore-head">
        <span className="tst-label">Restore</span>
        <Segmented label="Restore" value={mode} onChange={onMode} options={MODES} />
      </div>
      <PlanLines plan={plan} />
    </div>
  );
}
