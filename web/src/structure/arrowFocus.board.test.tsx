import { fireEvent, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { forgetViewports } from '../shell/viewport';
import { draftAgent, teamDraft, teamRecord } from '../test-fixtures';
import { saveTiming } from '../useTeamDraft';
import { card, fakeTeam, renderAt, resetApi } from './harness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  const names = ['getTeam', 'saveTeam', 'listTeams', 'templates', 'createTeam', 'startTeam', 'stopTeam', 'importTeam', 'listFolders', 'createFolder', 'listRigs', 'latestSeatEvents', 'stopRig'];
  return { ...m, api: { ...m.api, ...Object.fromEntries(names.map((n) => [n, vi.fn()])) } };
});

beforeEach(() => { resetApi(); forgetViewports(); localStorage.clear(); localStorage.setItem('sculptured.design.more-tools.v1', 'true'); saveTiming.delayMs = 5; });
afterEach(() => { saveTiming.delayMs = 500; });

/** 16 agents; agent 0 reports to the next 14, so the board has 14 arrows (more than QUIET_AFTER). */
function busy() {
  const agents = Array.from({ length: 16 }, (_, i) => draftAgent(`a${i}`, { name: `Agent ${i}`, x: 40 + (i % 4) * 300, y: 40 + Math.floor(i / 4) * 220 }));
  const connections = Array.from({ length: 14 }, (_, i) => ({ id: `c${i}`, from: 'a0', to: `a${i + 1}`, kind: 'reports-to' as const }));
  return teamRecord({ draft: teamDraft({ agents, connections }) });
}

const lit = () => document.querySelectorAll('path.edge.is-focus').length;

it('a busy board draws arrows faintly and brings forward the pointed-at card’s arrows', async () => {
  fakeTeam(busy());
  renderAt('/team/t/structure');
  await screen.findByRole('region', { name: 'Design board' });
  const canvas = document.querySelector('.sb-canvas')!;
  expect(canvas).toHaveClass('arrows-quiet');
  expect(lit()).toBe(0);
  fireEvent.pointerEnter(card('Agent 3').closest('.card-slot')!);
  expect(lit()).toBe(1);
  fireEvent.pointerEnter(card('Agent 0').closest('.card-slot')!);
  expect(lit()).toBe(14);
  await userEvent.click(screen.getByRole('button', { name: 'All arrows' }));
  expect(canvas).not.toHaveClass('arrows-quiet');
  expect(localStorage.getItem('sculptured.board.arrows')).toBe('all');
});
