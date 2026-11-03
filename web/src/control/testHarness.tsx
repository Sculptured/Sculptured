import { act, renderHook } from '@testing-library/react';
import type { ReactNode } from 'react';
import { vi } from 'vitest';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { RigSnapshot } from '../generated/RigSnapshot';
import { ShellContext, type RunHandle, type Shell } from '../shell/ShellContext';
import { ev, FakeEventSource, openFake } from '../test-fixtures';
import { ALL_TEAMS_STREAM, useTeams } from '../useTeams';
import { useDeploymentSource } from './deploymentSource';

/**
 * Test helpers for the deployment source: team `r` (agents a "Builder" and b) in a minimal shell.
 * Each test file mocks `../api` (listRigs, latestSeatEvents, teamQuestions, answer, teamUsage,
 * teamBudget, runUsage, teamControls, controlAgent, pauseTeam, resumeTeam, stopTeamNow, send).
 */

export const NOW = 60_000;
export const A = 'dev.a@r';
export const B = 'dev.b@r';
export const run: { current: RunHandle | null } = { current: null };

function Wrapper({ children }: { children: ReactNode }) {
  const { teams } = useTeams('r', openFake);
  const shell = { teams, names: { r: { [A]: 'Builder' } }, titles: { r: 'My team' }, now: NOW, refs: { run } } as unknown as Shell;
  return <ShellContext.Provider value={shell}>{children}</ShellContext.Provider>;
}

export const stream = () => FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM && !i.closed).at(-1)!;

/** The source for team `r`, with `snap` streamed and `history` as each agent's recent events. */
export async function open(snap: RigSnapshot, history: Record<string, EventRecord[]> = {}) {
  vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) => history[id] ?? []);
  const hook = renderHook(() => useDeploymentSource('r'), { wrapper: Wrapper });
  act(() => stream().emit('snapshot', snap));
  act(() => stream().emit('ready', {}));
  await act(async () => {});
  return hook;
}

export function question(id: string, over: Partial<QuestionView> = {}): QuestionView {
  return {
    id, seat: A, rig: 'r', source: 'approval', state: 'open', opened_at_ms: 5, deadline_ms: null, answer: null, answered_at_ms: null, late_answer: null,
    decision: null, withdrawn_reason: null, acknowledged: false, consequence: [], expired_seq: null, unconfirmed: null,
    prompt: { header: null, text: `Prompt ${id}`, options: [], allow_other: false, tool: null, tool_input: null }, ...over,
  };
}

/** Fresh fakes: nothing asked, some usage and cost, no control modes. */
export function resetFakes() {
  FakeEventSource.reset();
  vi.clearAllMocks();
  run.current = null;
  vi.mocked(api.listRigs).mockResolvedValue([]);
  vi.mocked(api.teamQuestions).mockResolvedValue([]);
  vi.mocked(api.teamUsage).mockResolvedValue({ agents: [{ seat: A, points: [{ at_ms: 1, input: 100, output: 50, cache_read: 0 }], reports: 1, context: null }] });
  vi.mocked(api.teamBudget).mockResolvedValue({ output_limit: null, agents: [], waiting: [] });
  const cost = { usd: 0.5, source: 'claude-code' as const };
  vi.mocked(api.runUsage).mockResolvedValue([ev(9, { type: 'usage-reported', turn_id: null, input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost })]);
  vi.mocked(api.teamControls).mockResolvedValue({});
}
