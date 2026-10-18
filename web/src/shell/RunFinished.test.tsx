import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { SeatView } from '../generated/SeatView';
import { agentState, vendor } from '../herdr';
import { seatView } from '../test-fixtures';
import type { AgentRow } from './model';
import { RunFinishedBar, RunSummary, SETTLE_MS } from './RunFinished';
import { allAgentsFinished, checkLine } from './runFinished';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, gitStatus: vi.fn() } };
});

function row(member: string, over: Partial<SeatView> = {}, lastText: string | null = `${member} says: all done`, turnEnd: number | null = 7): AgentRow {
  const seat = seatView(member, over);
  return {
    id: seat.id, rig: 'r', name: member[0].toUpperCase() + member.slice(1), group: 'dev', folder: '~', seat,
    state: agentState(seat, true), vendor: vendor(seat.runtime), summary: 'done', lastMs: null,
    activity: { chips: [], lastText, lastTurnEndSeq: turnEnd },
  };
}

describe('allAgentsFinished', () => {
  it('holds only when every agent is running, idle, has finished a turn and nothing waits', () => {
    expect(allAgentsFinished([row('a'), row('b')])).toBe(true);
    expect(allAgentsFinished([])).toBe(false);
    expect(allAgentsFinished([row('a'), row('b', { activity: 'working' })])).toBe(false);
    expect(allAgentsFinished([row('a'), row('b', { current_turn: 't9' })])).toBe(false);
    expect(allAgentsFinished([row('a'), row('b', {}, null, null)])).toBe(false); // never worked yet
    expect(allAgentsFinished([row('a'), row('b', { waiting_for: ['dev.a@r'] })])).toBe(false);
    expect(allAgentsFinished([row('a'), row('b', { open_questions: 1 })])).toBe(false);
    expect(allAgentsFinished([row('a'), row('b', { lifecycle: 'stopped' })])).toBe(false);
  });

  it('calls a check verified only when Sculptured ran it and it passed', () => {
    const check = (passed: boolean | null) => ({ command: 'npm test', passed, exit_code: passed ? 0 : 1, timed_out: false, output: '', at_ms: 1, duration_ms: 10 });
    expect(checkLine(row('a'))).toMatchObject({ verified: false, word: 'no check set · not verified' });
    expect(checkLine(row('a', { check: check(true) }))).toMatchObject({ verified: true, word: 'check passed: npm test' });
    expect(checkLine(row('a', { check: check(false) }))).toMatchObject({ verified: false, word: 'check failed: npm test (exit 1)' });
    expect(checkLine(row('a', { check: check(null) }))).toMatchObject({ verified: false });
  });
});

describe('Run summary (Results)', () => {
  beforeEach(() => vi.useFakeTimers({ shouldAdvanceTime: true }));
  afterEach(() => vi.useRealTimers());

  const show = (agents: AgentRow[], onOpen = vi.fn()) => {
    const r = render(<MemoryRouter><RunSummary rig="r" teamId="t" agents={agents} onOpen={onOpen} /></MemoryRouter>);
    return { ...r, onOpen };
  };

  it('says Run finished once every agent has stayed finished, and only what is known', async () => {
    vi.mocked(api.gitStatus).mockResolvedValue({ repository: true, branch: 'main', head: 'abc', files: [{ path: 'a.txt', index: ' ', worktree: 'M' }, { path: 'b.txt', index: '?', worktree: '?' }], log: '', fingerprint: 'f' });
    const { onOpen } = show([row('builder'), row('reviewer', {}, null)]);
    // Until the finish has held, it is not called finished.
    expect(screen.queryByRole('region', { name: 'Run finished' })).toBeNull();
    await act(async () => { vi.advanceTimersByTime(SETTLE_MS + 10); });
    const summary = screen.getByRole('region', { name: 'Run finished' });
    expect(within(summary).getByRole('banner')).toHaveTextContent('Every agent finished its turn; the team is still running.');
    const items = within(within(summary).getByRole('list', { name: 'What each agent did' })).getAllByRole('listitem');
    // Provenance is a small quote mark, its words for screen readers and on hover.
    expect(items[0]).toHaveTextContent('agent-reported');
    expect(items[0].querySelector('.ux-prov-mark')).toHaveAttribute('data-tip', expect.stringContaining('agent-reported'));
    expect(items[0].querySelector('.ux-prov-mark svg.icon')).not.toBeNull();
    expect(summary).not.toHaveTextContent(/[◇○◌✗]/);
    expect(items[0].querySelector('q')).toHaveAttribute('data-raw-evidence');
    expect(items[0]).toHaveTextContent('builder says: all done');
    expect(items[1]).toHaveTextContent('no reply captured');
    // No agent has a check: said once, in the verdict.
    const checks = screen.getByRole('region', { name: 'Checks' });
    expect(checks).toHaveTextContent('Verified: nothing. No agent has a check set');
    const files = screen.getByRole('region', { name: 'Files changed' });
    expect(await within(files).findByText(/2 files in the project folder/)).toBeInTheDocument();
    expect(files).toHaveTextContent('filesystem-observed');
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    await user.click(within(summary).getByRole('button', { name: 'Open Reviewer' }));
    expect(onOpen).toHaveBeenCalledWith('dev.reviewer@r');
    // Stop team lives in the project bar, once.
    expect(screen.queryByRole('button', { name: 'Stop team…' })).toBeNull();
  });

  it('while agents still work it is a run in progress, never "finished"', () => {
    vi.mocked(api.gitStatus).mockRejectedValue(new Error('no folder'));
    show([row('builder', { activity: 'working' }), row('reviewer')]);
    expect(screen.queryByRole('region', { name: 'Run finished' })).toBeNull();
    expect(screen.getByRole('region', { name: 'Run in progress' })).toHaveTextContent('What each agent reported so far');
  });

  it("lists each agent's check once any agent has one; a passing one is the only verified", async () => {
    vi.mocked(api.gitStatus).mockRejectedValue(new Error('no folder'));
    const passed = { command: 'npm test', passed: true, exit_code: 0, timed_out: false, output: '', at_ms: 1, duration_ms: 10 };
    show([row('builder', { check: passed }), row('reviewer')]);
    const checks = screen.getByRole('region', { name: 'Checks' });
    expect(checks).toHaveTextContent('check passed: npm test');
    expect(checks).toHaveTextContent('no check set · not verified');
    expect(checks).toHaveTextContent('Verified: 1 of 2 agents passed the check Sculptured ran.');
  });

  it('says files changed are unknown when the folder cannot be read', async () => {
    vi.mocked(api.gitStatus).mockRejectedValue(new Error('no folder'));
    show([row('builder')]);
    expect(await within(screen.getByRole('region', { name: 'Files changed' })).findByText('unknown')).toBeInTheDocument();
  });
});

describe('Run finished bar (Run board)', () => {
  beforeEach(() => vi.useFakeTimers({ shouldAdvanceTime: true }));
  afterEach(() => vi.useRealTimers());

  it('appears once every agent has stayed finished, as one honest line that leads to Results', async () => {
    const { rerender } = render(<MemoryRouter><RunFinishedBar agents={[row('builder'), row('reviewer')]} href="/p/r/results" /></MemoryRouter>);
    expect(screen.queryByRole('link')).toBeNull();
    await act(async () => { vi.advanceTimersByTime(SETTLE_MS + 10); });
    const bar = screen.getByRole('link', { name: /View results/ });
    expect(bar).toHaveTextContent('Run finished · 2 reported done · 0 verified');
    expect(bar).toHaveAttribute('href', '/p/r/results');
    rerender(<MemoryRouter><RunFinishedBar agents={[row('builder', { activity: 'working' })]} href="/p/r/results" /></MemoryRouter>);
    expect(screen.queryByRole('link')).toBeNull();
  });
});
