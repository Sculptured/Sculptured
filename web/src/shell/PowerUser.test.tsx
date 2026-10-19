import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AppRoutes } from '../App';
import { api } from '../api';
import { resetSeenCache } from '../seen';
import { FakeEventSource, openFake, seatView, snapshot } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { ALL_TEAMS_STREAM } from '../useTeams';
import { forgetViewports } from './LiveGraph';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn(), teamRecent: vi.fn(), listRigs: vi.fn(), interrupt: vi.fn(), send: vi.fn(), stopRig: vi.fn() } };
});

function Where() {
  return <div data-testid="where">{decodeURIComponent(useLocation().pathname)}</div>;
}
const where = () => screen.getByTestId('where').textContent;

/** Team r with running agents a and b, and a stopped agent c. */
async function team(url = '/rig/r') {
  vi.mocked(api.listRigs).mockResolvedValue([{ name: 'r', desired: 'running', seat_count: 3, running_count: 2, attention_count: 0 }]);
  render(
    <MemoryRouter initialEntries={[url]}>
      <AppRoutes openStream={openFake} />
      <Where />
    </MemoryRouter>,
  );
  await waitFor(() => expect(api.listRigs).toHaveBeenCalled());
  await act(async () => {});
  const base = snapshot();
  const rig = { ...base.rig, seats: [...base.rig.seats, { ...base.rig.seats[1], id: 'dev.c@r', member: 'c' }] };
  const s = FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM && !i.closed).at(-1)!;
  act(() => s.emit('snapshot', snapshot({ rig, seats: [seatView('a'), seatView('b'), seatView('c', { lifecycle: 'stopped' })] })));
}

beforeEach(() => {
  FakeEventSource.reset();
  forgetViewports();
  localStorage.clear();
  resetSeenCache();
  vi.mocked(api.latestSeatEvents).mockReset().mockResolvedValue([]);
  vi.mocked(api.teamRecent).mockReset().mockResolvedValue({});
  vi.mocked(api.send).mockReset().mockResolvedValue({ message_id: 'm1' });
});

describe('Run view: pick agents and message them together', () => {
  it('Shift+click picks agents instead of opening them, and the bar messages exactly those', async () => {
    await team();
    const user = userEvent.setup();
    const graph = screen.getByTestId('graph-world');
    await user.keyboard('{Shift>}');
    await user.click(within(graph).getByRole('button', { name: 'a — idle' }));
    await user.click(within(graph).getByRole('button', { name: 'b — idle' }));
    await user.keyboard('{/Shift}');
    expect(where()).toBe('/p/r/board');
    const bar = screen.getByRole('toolbar', { name: '2 agents selected' });
    expect(graph.querySelectorAll('.agent-box.picked')).toHaveLength(2);
    await user.click(within(bar).getByRole('button', { name: 'Message…' }));
    const dialog = screen.getByRole('dialog', { name: /^Message agents/ });
    expect(within(dialog).getAllByRole('checkbox').map((c) => (c as HTMLInputElement).checked)).toEqual([true, true, false]);
    expect(openRigWords(dialog)).toEqual([]);
    await user.keyboard('status please');
    await user.click(within(dialog).getByRole('button', { name: 'Send to 2 agents…' }));
    await user.click(screen.getByRole('button', { name: 'Send to 2 agents' }));
    await screen.findByText('Sent to 2 agents.');
    expect(vi.mocked(api.send).mock.calls.map((c) => c[0])).toEqual(['dev.a@r', 'dev.b@r']);
    await user.click(screen.getAllByRole('button', { name: 'Close' }).at(-1)!);
    // Esc lets go of the picked agents.
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('toolbar', { name: /selected$/ })).toBeNull();
  });
});

describe('Command palette in the app', () => {
  it('Message all agents ticks every running agent', async () => {
    await team();
    const user = userEvent.setup();
    await user.keyboard('{Control>}k{/Control}');
    await user.keyboard('message all{Enter}');
    const dialog = screen.getByRole('dialog', { name: /^Message agents/ });
    expect(within(dialog).getAllByRole('checkbox').map((c) => (c as HTMLInputElement).checked)).toEqual([true, true, false]);
  });

  it('Message selected agents says how to select when none is', async () => {
    await team();
    const user = userEvent.setup();
    await user.keyboard('{Control>}k{/Control}');
    await user.keyboard('message selected');
    const row = screen.getByRole('option', { name: 'Message selected agents…' });
    expect(row).toHaveAccessibleDescription('Select agents first: Shift+click them.');
    // Every command, in plain words.
    await user.clear(screen.getByRole('combobox'));
    await user.keyboard('>');
    expect(screen.getAllByRole('option').length).toBeGreaterThan(10);
    // Community's "profile" is the plain word, as on its pages.
    expect(openRigWords(screen.getByRole('dialog', { name: 'Command palette' }), ['profile', 'profiles'])).toEqual([]);
  });

  it('goes to every page of the project, and Stop team… opens the same confirmation as the button', async () => {
    await team();
    const user = userEvent.setup();
    for (const [say, to] of [['open agents', '/p/r/agents'], ['open results', '/p/r/results'], ['open settings', '/p/r/settings'], ['open run history', '/p/r/results/history'], ['board: run', '/p/r/board']]) {
      await user.keyboard('{Control>}k{/Control}');
      await user.keyboard(`${say}{Enter}`);
      await waitFor(() => expect(where()).toBe(to));
    }
    await user.keyboard('{Control>}k{/Control}');
    await user.keyboard('stop team{Enter}');
    expect(screen.getByRole('group', { name: 'Confirm stop team' })).toBeInTheDocument();
    expect(api.stopRig).not.toHaveBeenCalled();
  });

  it('] and [ go to the next and previous agent of the team', async () => {
    await team('/rig/r/agent/dev.a@r');
    const user = userEvent.setup();
    (document.activeElement as HTMLElement | null)?.blur();
    await user.keyboard(']');
    expect(where()).toBe('/p/r/agents/dev.b');
    // '[[' is one '[' press in user-event's key syntax.
    await user.keyboard('[[');
    expect(where()).toBe('/p/r/agents/dev.a');
    await user.keyboard('[[');
    expect(where()).toBe('/p/r/agents/dev.c');
  });
});
