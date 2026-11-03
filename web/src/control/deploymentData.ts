import { useCallback } from 'react';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { TeamUsage } from '../generated/TeamUsage';
import { usePolled } from '../usePolling';
import type { Modes } from './deploymentAgents';
import type { Approval, ControlSource } from './source';

/** What a deployment source polls: questions, usage, the run budget, cost reports and control modes. */

const QUESTIONS_MS = 15_000;
const USAGE_MS = 10_000;
const COST_MS = 30_000;
const MODES_MS = 5_000;

/** Each agent's control mode; an agent whose mode could not be read stays unknown (undefined). */
async function controlModes(key: string): Promise<Modes> {
  const ids = key ? key.split('|') : [];
  const got = await Promise.allSettled(ids.map((id) => api.agentControl(id)));
  return Object.fromEntries(ids.map((id, i) => {
    const r = got[i];
    return [id, r.status === 'fulfilled' ? r.value?.mode ?? null : undefined];
  }));
}

export function useDeploymentData(team: string, seatIds: string[], events: EventRecord[]) {
  // A question event (asked, answered, expired…) reloads the questions at once.
  const asked = events.filter((e) => e.body.type.startsWith('question')).length;
  const questions = usePolled(() => api.teamQuestions(team), QUESTIONS_MS, [team, asked]);
  const usage = usePolled(() => api.teamUsage(team), USAGE_MS, [team]);
  const budget = usePolled(() => api.teamBudget(team), USAGE_MS, [team]);
  const cost = usePolled(() => api.runUsage(team), COST_MS, [team]);
  const key = seatIds.join('|');
  const modes = usePolled(() => controlModes(key), MODES_MS, [key]);
  const reloadQuestions = questions.reload;
  const reloadModes = modes.reload;
  const reload = useCallback(() => { reloadQuestions(); reloadModes(); }, [reloadQuestions, reloadModes]);
  const problems = [
    questions.error && `Could not load the questions: ${questions.error}`,
    usage.error && `Could not load token use: ${usage.error}`,
    budget.error && `Could not load the run budget: ${budget.error}`,
    cost.error && `Could not load the cost reports: ${cost.error}`,
  ].filter((p): p is string => !!p);
  return { questions: questions.data ?? [], usage: usage.data, budget: budget.data, costEvents: cost.data, modes: modes.data ?? {}, reload, problems };
}

/** Questions that take approve or reject, open now. The others are answered in words, in Needs you. */
export function approvalsOf(questions: QuestionView[]): Approval[] {
  return questions
    .filter((q) => q.state === 'open' && (q.source === 'approval' || q.source === 'native-permission'))
    .map((q) => {
      const title = q.prompt.header?.trim() || q.prompt.text.split('\n')[0];
      return { id: q.id, agent: q.seat, title, ...(q.prompt.text !== title ? { detail: q.prompt.text } : {}), kind: q.source === 'approval' ? 'change' : 'question' };
    });
}

/**
 * Tokens from each agent's latest usage total; cost from the latest run's final usage reports, only
 * when every one of them was priced (otherwise unknown). Tool calls per minute come from the caller.
 */
export function metricsOf(usage: TeamUsage | null, costEvents: EventRecord[] | null, toolCallsPerMin: number[]): ControlSource['metrics'] {
  const last = (usage?.agents ?? []).flatMap((a) => (a.points.length > 0 ? [a.points[a.points.length - 1]] : []));
  const tokens = last.length > 0 ? { tokensIn: sum(last.map((p) => p.input)), tokensOut: sum(last.map((p) => p.output)) } : {};
  const finals = (costEvents ?? []).flatMap((e) => (e.body.type === 'usage-reported' && !e.body.in_turn ? [e.body.cost] : []));
  const priced = finals.length > 0 && finals.every((c) => c !== undefined);
  return { ...tokens, ...(priced ? { costUsd: sum(finals.map((c) => c?.usd ?? 0)) } : {}), toolCallsPerMin };
}

const sum = (xs: number[]) => xs.reduce((n, x) => n + x, 0);
