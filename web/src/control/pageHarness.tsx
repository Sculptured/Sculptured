import { act, screen, within } from '@testing-library/react';
import { vi } from 'vitest';
import { api } from '../api';
import type { EventRecord } from '../generated/EventRecord';
import { resetSeenCache } from '../seen';
import { forgetViewports } from '../shell/LiveGraph';
import { twoTeams } from '../shell/shellHarness';
import { ev, snapshot } from '../test-fixtures';
import { FakeChannel } from '../workspace/testkit';
import { resetFakes } from './testHarness';

/**
 * Test helpers for the Control page in the app frame (team r, agents a and b). Each test file mocks
 * `../api` itself (vi.mock is hoisted per file) with these calls: listRigs, latestSeatEvents,
 * teamQuestions, answer, teamUsage, teamBudget, runUsage, teamControls, controlAgent, agentControl,
 * pauseTeam, resumeTeam, stopTeamNow, send, machineMetrics, runCommunications, managedChildren,
 * teamRecent, deployments, seatQuestions; and calls `resetControl` before each test.
 */

export const A = 'dev.a@r';
export const turn = (seq: number, type: 'turn-started' | 'turn-ended', seat = A): EventRecord =>
  ev(seq, type === 'turn-started' ? { type, turn_id: `t${seq}` } as EventRecord['body'] : { type, turn_id: `t${seq}`, status: 'completed' } as EventRecord['body'], seat);

export function resetControl() {
  resetFakes();
  forgetViewports();
  localStorage.clear();
  resetSeenCache();
  FakeChannel.open.clear();
  vi.stubGlobal('BroadcastChannel', FakeChannel);
  vi.mocked(api.latestSeatEvents).mockImplementation(async (id: string) => (id === A ? [turn(1, 'turn-started'), turn(2, 'turn-ended')] : []));
  vi.mocked(api.machineMetrics).mockResolvedValue([]);
  vi.mocked(api.runCommunications).mockResolvedValue([]);
  vi.mocked(api.managedChildren).mockResolvedValue([]);
  vi.mocked(api.agentControl).mockResolvedValue({ mode: null } as never);
  vi.mocked(api.teamRecent).mockResolvedValue({});
  vi.mocked(api.deployments).mockResolvedValue([]);
  vi.mocked(api.seatQuestions).mockResolvedValue([]);
}

export const pane = (title: string) => screen.getByRole('region', { name: title });
export const layoutTab = (name: string) => within(screen.getByRole('tablist', { name: 'Layouts' })).getByRole('tab', { name });
export const paneTab = (title: string) => screen.getByRole('tab', { name: new RegExp(title) });
export const header = () => screen.getByRole('group', { name: 'Team controls' });

/** The app at team r's Control page, with `snap` streamed. */
export async function control(snap = snapshot()) {
  await twoTeams('/p/r/control', snap);
  await act(async () => {});
}
