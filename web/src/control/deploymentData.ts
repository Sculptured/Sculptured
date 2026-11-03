import { useCallback, useMemo } from 'react';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { TeamControls } from '../generated/TeamControls';
import type { TeamUsage } from '../generated/TeamUsage';
import { usePolled } from '../usePolling';
import type { Modes } from './deploymentAgents';
import type { Approval, ControlSource } from './source';

/** What a deployment source polls: questions, usage, the run budget, cost reports and control modes. */

const QUESTIONS_MS = 15_000;
const USAGE_MS = 10_000;
const COST_MS = 30_000;
/** A safety net: modes also reload on every status change and after each action. */
const MODES_MS = 30_000;

/**
 * `load`, never twice at once: a call while one runs waits for it, then loads once more (so what an
 * action just changed is read), however many calls came meanwhile.
 */
export function oneAtATime<T>(load: () => Promise<T>): () => Promise<T> {
  let current: Promise<T> | null = null;
  let next: Promise<T> | null = null;
  const run = (): Promise<T> => {
    if (!current) {
      current = load().finally(() => { current = null; });
      return current;
    }
    next ??= current.then(() => undefined, () => undefined).then(() => { next = null; return run(); });
    return next;
  };
  return run;
}

/** Every agent's control mode, from one team request; an agent absent from it has none (null). */
function modesOf(seatIds: string[], controls: TeamControls | null): Modes {
  if (!controls) return {};
  return Object.fromEntries(seatIds.map((id) => [id, controls[id] ?? null]));
}

/** The latest status change in the list: a new one reloads the control modes at once. */
const lastStatusChange = (events: EventRecord[]) => events.filter((e) => e.body.type === 'seat-lifecycle-changed').at(-1)?.seq ?? 0;

export function useDeploymentData(team: string, seatIds: string[], events: EventRecord[]) {
  // A question event (asked, answered, expired…) reloads the questions at once.
  const asked = events.filter((e) => e.body.type.startsWith('question')).length;
  const questions = usePolled(() => api.teamQuestions(team), QUESTIONS_MS, [team, asked]);
  const usage = usePolled(() => api.teamUsage(team), USAGE_MS, [team]);
  const budget = usePolled(() => api.teamBudget(team), USAGE_MS, [team]);
  const cost = usePolled(() => api.runUsage(team), COST_MS, [team]);
  const loadControls = useMemo(() => oneAtATime(() => api.teamControls(team)), [team]);
  const controls = usePolled(loadControls, MODES_MS, [team, lastStatusChange(events)]);
  const reloadQuestions = questions.reload;
  const reloadModes = controls.reload;
  const reload = useCallback(() => { reloadQuestions(); reloadModes(); }, [reloadQuestions, reloadModes]);
  const problems = [
    questions.error && `Could not load the questions: ${questions.error}`,
    usage.error && `Could not load token use: ${usage.error}`,
    budget.error && `Could not load the run budget: ${budget.error}`,
    cost.error && `Could not load the cost reports: ${cost.error}`,
    controls.error && `Could not load which agents are paused or stopping: ${controls.error}`,
  ].filter((p): p is string => !!p);
  return { questions: questions.data ?? [], usage: usage.data, budget: budget.data, costEvents: cost.data, modes: modesOf(seatIds, controls.data), reload, problems };
}

const KIND: Record<QuestionView['source'], Approval['kind']> = {
  approval: 'change', 'native-permission': 'question', 'request-input': 'answer', 'native-question': 'answer',
};

/**
 * Every open question: changes and tool requests take approve or reject; the others (`answer`) need a
 * written answer, given in Needs you. A tool request's title names the tool.
 */
export function approvalsOf(questions: QuestionView[]): Approval[] {
  return questions
    .filter((q) => q.state === 'open')
    .map((q) => {
      const first = q.prompt.header?.trim() || q.prompt.text.split('\n')[0];
      const title = q.source === 'native-permission' && q.prompt.tool ? `Use ${q.prompt.tool}: ${first}` : first;
      return { id: q.id, agent: q.seat, title, ...(q.prompt.text !== title ? { detail: q.prompt.text } : {}), kind: KIND[q.source] };
    });
}

/**
 * Tokens from each agent's latest usage total; cost from the latest run's final usage reports, only
 * when every one of them was priced (otherwise unknown). Tool calls per minute come from the caller.
 */
export function metricsOf(usage: TeamUsage | null, costEvents: EventRecord[] | null, toolCallsPerMin: (number | null)[]): ControlSource['metrics'] {
  const last = (usage?.agents ?? []).flatMap((a) => (a.points.length > 0 ? [a.points[a.points.length - 1]] : []));
  const tokens = last.length > 0 ? { tokensIn: sum(last.map((p) => p.input)), tokensOut: sum(last.map((p) => p.output)) } : {};
  const finals = (costEvents ?? []).flatMap((e) => (e.body.type === 'usage-reported' && !e.body.in_turn ? [e.body.cost] : []));
  const priced = finals.length > 0 && finals.every((c) => c !== undefined);
  return { ...tokens, ...(priced ? { costUsd: sum(finals.map((c) => c?.usd ?? 0)) } : {}), toolCallsPerMin };
}

const sum = (xs: number[]) => xs.reduce((n, x) => n + x, 0);
