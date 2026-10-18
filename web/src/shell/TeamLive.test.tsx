import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AppRoutes } from '../App';
import { api, ApiError } from '../api';
import { ev, FakeEventSource, openFake, seatView, snapshot } from '../test-fixtures';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, latestSeatEvents: vi.fn().mockResolvedValue([]), listRigs: vi.fn().mockResolvedValue([]), stopRig: vi.fn(), stopTeamNow: vi.fn() } };
});

function Where() {
  return <div data-testid="where">{decodeURIComponent(useLocation().pathname)}</div>;
}
function renderAt(url: string) {
  render(
    <MemoryRouter initialEntries={[url]}>
      <AppRoutes openStream={openFake} />
      <Where />
    </MemoryRouter>,
  );
}

describe('RigView', () => {
  beforeEach(() => FakeEventSource.reset());

  it('shows a connecting state until the first snapshot', () => {
    renderAt('/rig/r');
    expect(screen.getByRole('status')).toHaveTextContent('Connecting to r');
  });

  it('Enter on a seat card navigates to the seat route', async () => {
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    screen.getByRole('button', { name: 'a — idle' }).focus();
    await userEvent.setup().keyboard('{Enter}');
    expect(screen.getByTestId('where')).toHaveTextContent('/p/r/agents/dev.a');
    expect(screen.getByRole('region', { name: 'Agent a' })).toBeInTheDocument();
  });

  it('the Agents tab is the list fallback: every agent as a table row, and its arrows as sentences', async () => {
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    await userEvent.setup().click(screen.getByRole('link', { name: 'Agents' }));
    expect(screen.queryByTestId('graph-world')).toBeNull();
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    expect(rows.map((r) => r.querySelector('a')?.textContent)).toEqual(['a', 'b']);
    const links = screen.getByRole('region', { name: 'Connections' });
    expect(links).toHaveTextContent('a hands work to b');
    expect(links).toHaveTextContent('b watches a (not enforced)');
  });

  it("shows an agent's full reason in the Agents list, as raw evidence", async () => {
    renderAt('/p/r/agents');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ seats: [seatView('a', { lifecycle: 'failed', lifecycle_reason: 'boom' }), seatView('b')] })));
    expect(await screen.findByText('boom')).toHaveAttribute('data-raw-evidence');
  });

  it('shows a pluralised gap line as a status', () => {
    renderAt('/rig/r');
    const src = FakeEventSource.instances[0];
    act(() => src.emit('snapshot', snapshot()));
    const lost = (seq: number, count: number) => ({ ...ev(seq, { type: 'events-lost', count }), host: 'controld', subject: { kind: 'host', id: 'local' } });
    act(() => src.emit('event', { event: lost(11, 1), seat: null }));
    expect(screen.getByText('1 event lost — history has a gap')).toHaveAttribute('role', 'status');
  });

  it('pluralises several lost events', () => {
    renderAt('/rig/r');
    const src = FakeEventSource.instances[0];
    act(() => src.emit('snapshot', snapshot()));
    const lost = { ...ev(11, { type: 'events-lost', count: 3 }), host: 'controld', subject: { kind: 'host', id: 'local' } };
    act(() => src.emit('event', { event: lost, seat: null }));
    expect(screen.getByText('3 events lost — history has a gap')).toBeInTheDocument();
  });

  it('distinguishes a seat outside the spec from a seat with unknown status', () => {
    renderAt('/rig/r/agent/dev.zzz@r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ seats: [seatView('a')] })));
    expect(screen.getByRole('alert')).toHaveTextContent('is not part of r');
  });

  it('says status is unknown for a spec seat with no view yet', () => {
    renderAt('/rig/r/agent/dev.b@r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ seats: [seatView('a')] })));
    expect(screen.getByRole('alert')).toHaveTextContent('is part of this team but its status is unknown');
  });
});

// I4: compiled-rig diagnostics reach the rig header.
describe('RigView spec notes', () => {
  beforeEach(() => FakeEventSource.reset());
  const warning = {
    path: 'permission_policy',
    severity: 'warning' as const,
    message: 'permission_policy is recorded but not enforced until M2; the seat runs with the default unrestricted posture',
  };
  const notEnforced = { path: 'culture_file', severity: 'not-enforced' as const, message: 'declared · not enforced in this version' };
  const error = { path: 'x', severity: 'error' as const, message: 'never shown here' };

  /** Spec notes are part of Results, under Advanced. */
  function openWith(diagnostics: (typeof warning | typeof notEnforced | typeof error)[]) {
    renderAt('/p/r/results');
    const rig = { ...snapshot().rig, diagnostics };
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ rig })));
  }

  it('shows each warning and not-enforced diagnostic with its severity in words, path and message', () => {
    openWith([warning, notEnforced]);
    const list = screen.getByRole('list', { name: 'Spec notes' });
    const items = within(list).getAllByRole('listitem').map((li) => li.textContent);
    expect(items).toEqual([
      `warning · permission_policy — ${warning.message}`,
      'declared · not enforced · culture_file — declared · not enforced in this version',
    ]);
    expect(list.closest('details')).toHaveAttribute('open');
    expect(screen.getByText('Spec notes: 1 warning, 1 declared · not enforced')).toBeInTheDocument();
  });

  // A native <summary> is keyboard-operable in browsers; jsdom only implements its click activation.
  it('collapses more than three notes behind a summary that expands them', async () => {
    openWith([warning, notEnforced, { ...notEnforced, path: 'pods[0].startup' }, { ...notEnforced, path: 'pods[0].continuity_policy' }]);
    const summary = screen.getByText('Spec notes: 1 warning, 3 declared · not enforced');
    const details = summary.closest('details')!;
    expect(details).not.toHaveAttribute('open');
    await userEvent.setup().click(summary);
    expect(details).toHaveAttribute('open');
  });

  it('shows nothing when the spec has no warnings', () => {
    openWith([]);
    expect(screen.queryByRole('list', { name: 'Spec notes' })).not.toBeInTheDocument();
  });
});

describe('RigView stop rig', () => {
  beforeEach(() => {
    FakeEventSource.reset();
    vi.mocked(api.stopRig).mockReset();
  });
  function open() {
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
  }
  const deferred = () => {
    let resolve!: () => void;
    let reject!: (e: unknown) => void;
    const promise = new Promise<never>((res, rej) => {
      resolve = res as () => void;
      reject = rej;
    });
    return { promise, resolve, reject };
  };

  it('opens an inline confirm, never window.confirm, and Cancel closes it with focus restored', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm');
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    expect(screen.getByRole('group', { name: 'Confirm stop team' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('group', { name: 'Confirm stop team' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop team…' })).toHaveFocus();
    expect(confirmSpy).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it('Escape cancels and returns focus', async () => {
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('group', { name: 'Confirm stop team' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop team…' })).toHaveFocus();
  });

  it('disables confirm with a visible reason while in flight, then closes and restores focus', async () => {
    const d = deferred();
    vi.mocked(api.stopRig).mockReturnValue(d.promise as never);
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    expect(api.stopRig).toHaveBeenCalledWith('r', expect.any(String));
    expect(screen.getByRole('button', { name: 'Confirm stop team' })).toBeDisabled();
    expect(screen.getByText('Stopping r…')).toBeVisible();
    await act(async () => d.resolve());
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Confirm stop team' })).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Stop team…' })).toHaveFocus();
  });

  it('offers Stop everything now beside the normal stop, and it stops the team at once', async () => {
    vi.mocked(api.stopTeamNow).mockResolvedValue({ stopped: 2 });
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Stop everything now' }));
    expect(api.stopTeamNow).toHaveBeenCalledWith('r');
    expect(api.stopRig).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Confirm stop team' })).not.toBeInTheDocument());
  });

  it('reuses the request id after a network failure', async () => {
    vi.mocked(api.stopRig).mockRejectedValueOnce(new TypeError('network')).mockResolvedValueOnce(undefined as never);
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    await screen.findByRole('alert');
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    const [first, second] = vi.mocked(api.stopRig).mock.calls;
    expect(second[1]).toBe(first[1]);
  });

  it('uses a new request id after the server answered with an error', async () => {
    vi.mocked(api.stopRig).mockRejectedValueOnce(new ApiError(409, 'conflict', 'busy')).mockResolvedValueOnce(undefined as never);
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    await screen.findByRole('alert');
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    const [first, second] = vi.mocked(api.stopRig).mock.calls;
    expect(second[1]).not.toBe(first[1]);
  });

  // I7a: stop rig reports each seat it could not stop, with the host's reason.
  it('lists the seats it could not stop, with their reasons, as an alert', async () => {
    vi.mocked(api.stopRig).mockResolvedValue({
      failed: [
        { seat: 'dev.b@r', reason: "Sculptured's agent runner is not connected" },
        { seat: 'dev.a@r', reason: 'host did not reply within 30s' },
      ],
    });
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('2 agents could not be stopped');
    const items = screen.getAllByRole('listitem').filter((li) => alert.contains(li));
    expect(items.map((li) => li.textContent)).toEqual([
      "b — Sculptured's agent runner is not connected",
      'a — host did not reply within 30s',
    ]);
  });

  it('shows no failure alert when every seat stopped', async () => {
    vi.mocked(api.stopRig).mockResolvedValue({ failed: [] });
    open();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Confirm stop team' })).not.toBeInTheDocument());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('redirects old agent links to the agent page', async () => {
    renderAt('/rig/r/seat/dev.a%40r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    expect(screen.getByTestId('where')).toHaveTextContent('/p/r/agents/dev.a');
    expect(screen.getByRole('region', { name: 'Agent a' })).toBeInTheDocument();
  });

  it('keeps the tab when redirecting an old seat link', async () => {
    renderAt('/rig/r/seat/dev.a%40r/events');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    expect(screen.getByTestId('where')).toHaveTextContent('/p/r/agents/dev.a/activity');
    expect(screen.getByRole('button', { name: 'Raw events' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('sends a bare old seat link to the team instead of looping', async () => {
    renderAt('/rig/r/seat');
    expect(screen.getByTestId('where').textContent).toBe('/p/r/board');
    expect(screen.getByRole('status')).toHaveTextContent('Connecting to r');
  });

  it('opens a team\'s board from /team/:id/live', () => {
    renderAt('/team/r/live');
    expect(screen.getByTestId('where').textContent).toBe('/p/r/board');
    expect(screen.getByRole('status')).toHaveTextContent('Connecting to r');
  });

  it('sends an unknown address home instead of a blank page', () => {
    renderAt('/no/such/page');
    expect(screen.getByTestId('where').textContent).toBe('/');
  });
});
