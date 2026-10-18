import { act, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { expect, vi } from 'vitest';
import { AppRoutes } from '../App';
import { api } from '../api';
import type { CompiledRig } from '../generated/CompiledRig';
import type { RigSnapshot } from '../generated/RigSnapshot';
import type { SeatView } from '../generated/SeatView';
import { compiledRig, FakeEventSource, openFake, seatView, snapshot } from '../test-fixtures';
import { ALL_TEAMS_STREAM } from '../useTeams';

/** Test helpers for the app frame: two teams, r (agents a, b) and q (agents x, y in group Ops). */

function Where() {
  const l = useLocation();
  return <div data-testid="where">{decodeURIComponent(l.pathname) + l.search}</div>;
}

export function renderApp(url: string) {
  return render(
    <MemoryRouter initialEntries={[url]}>
      <AppRoutes openStream={openFake} />
      <Where />
    </MemoryRouter>,
  );
}
export const where = () => screen.getByTestId('where').textContent;

/** Team `q`: group ops with agents x and y, each in its own folder. */
export function rigQ(): CompiledRig {
  const base = compiledRig();
  const seat = (m: string) => ({ ...base.seats[0], id: `ops.${m}@q`, pod: 'ops', member: m, cwd: `/work/${m}-folder` });
  return { ...base, name: 'q', pods: [{ id: 'ops', label: 'Ops' }], seats: [seat('x'), seat('y')], edges: [], launch_layers: [['ops.x@q', 'ops.y@q']] };
}
export function viewQ(m: string, over: Partial<SeatView> = {}): SeatView {
  return { ...seatView(m), id: `ops.${m}@q`, rig: 'q', pod: 'ops', ...over };
}
export function snapQ(over: Partial<RigSnapshot> = {}): RigSnapshot {
  return { rig: rigQ(), desired: 'running', seats: [viewQ('x'), viewQ('y')], last_seq: 5, ...over };
}
export const summary = (name: string, attention = 0) => ({ name, desired: 'running' as const, seat_count: 2, running_count: 2, attention_count: attention });

/** The one live connection every team shares (the latest one open). Snapshots name their team. */
export function stream() {
  const s = FakeEventSource.instances.filter((i) => i.url === ALL_TEAMS_STREAM && !i.closed).at(-1);
  if (!s) throw new Error('no live stream');
  return s;
}

/** Teams r and q, both running, with the app open at `url`. */
export async function twoTeams(url = '/rig/r', r: RigSnapshot = snapshot(), q: RigSnapshot = snapQ()) {
  vi.mocked(api.listRigs).mockResolvedValue([summary('r'), summary('q')]);
  renderApp(url);
  await waitFor(() => expect(api.listRigs).toHaveBeenCalled());
  await act(async () => {});
  act(() => stream().emit('snapshot', r));
  act(() => stream().emit('snapshot', q));
}
