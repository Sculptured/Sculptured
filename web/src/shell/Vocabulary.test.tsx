import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AppRoutes } from '../App';
import { api } from '../api';
import { compiledRig, ev, FakeEventSource, openFake, seatView, snapshot } from '../test-fixtures';
import { EventsTab } from '../components/EventsTab';
import { openRigWords } from '../test-vocab';

// The factory is hoisted above the imports, so it must not call `ev` (an import).
// The events are set in beforeEach instead.
vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return {
    ...m,
    api: {
      ...m.api,
      listRigs: vi.fn().mockResolvedValue([]),
      latestSeatEvents: vi.fn(),
      stopRig: vi.fn(),
    },
  };
});

describe('vocabulary', () => {
  beforeEach(() => {
    FakeEventSource.reset();
    vi.mocked(api.stopRig).mockReset();
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'message-queued', message_id: 'm1', intent: 'inform', text: 'hello' }),
      ev(2, { type: 'tenure-ended', tenure: 1, exit_code: 0, signal: null, stderr_tail: '' }),
    ]);
  });

  it('the run view and inspector use no OpenRig words outside Advanced', async () => {
    render(
      <MemoryRouter initialEntries={['/rig/r/agent/dev.a%40r']}>
        <AppRoutes openStream={openFake} />
      </MemoryRouter>,
    );
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    await screen.findByText('hello');
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('a stopped agent and a spec warning add no OpenRig words outside Advanced', async () => {
    render(
      <MemoryRouter initialEntries={['/rig/r/agent/dev.a%40r']}>
        <AppRoutes openStream={openFake} />
      </MemoryRouter>,
    );
    const rig = {
      ...compiledRig(),
      diagnostics: [
        {
          path: 'pods[0].members[0].permission_policy',
          severity: 'warning' as const,
          message: 'permission_policy is recorded but not enforced until M2; the seat runs with the default unrestricted posture',
        },
      ],
    };
    act(() =>
      FakeEventSource.instances[0].emit('snapshot', snapshot({ rig, seats: [seatView('a', { lifecycle: 'stopped' }), seatView('b')] })),
    );
    // Controls live on the agent's Activity tab.
    await userEvent.click(await screen.findByRole('link', { name: 'Activity' }));
    expect(await screen.findByRole('button', { name: 'Hard stop…' })).toHaveAccessibleDescription('Status: stopped; there is no process to stop.');
    expect(openRigWords(document.body)).toEqual([]);
  });

  function renderAt(url: string) {
    render(
      <MemoryRouter initialEntries={[url]}>
        <AppRoutes openStream={openFake} />
      </MemoryRouter>,
    );
  }

  it('the Events tab uses no OpenRig words outside raw evidence', async () => {
    renderAt('/rig/r/agent/dev.a%40r/events');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    await screen.findByText('tenure-ended');
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('the real EventsTab table is marked as raw evidence', async () => {
    render(<EventsTab seatId="dev.a@r" feed={[]} epoch={0} />);
    const table = await screen.findByRole('table', { name: 'Raw events' });
    expect(table).toHaveAttribute('data-raw-evidence');
  });

  it('the open stop-agent confirm uses no OpenRig words', async () => {
    renderAt('/p/r/agents/dev.a/activity');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Hard stop…' }));
    expect(screen.getByRole('group', { name: 'Confirm stop agent' })).toBeInTheDocument();
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('the stop-failure alert keeps host reasons verbatim and adds no OpenRig words', async () => {
    vi.mocked(api.stopRig).mockResolvedValue({ failed: [{ seat: 'dev.b@r', reason: 'no seat process found on host local' }] });
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Stop team…' }));
    await user.click(screen.getByRole('button', { name: 'Confirm stop team' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('b — no seat process found on host local');
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('an agent that is not in the team adds no OpenRig words', async () => {
    renderAt('/rig/r/agent/dev.zzz%40r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot()));
    expect(await screen.findByRole('alert')).toHaveTextContent('zzz is not part of r.');
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('an agent in the team with no status adds no OpenRig words', async () => {
    renderAt('/rig/r/agent/dev.b%40r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ seats: [seatView('a')] })));
    expect(await screen.findByRole('alert')).toHaveTextContent('b is part of this team but its status is unknown.');
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('the Agents list with nothing selected adds no OpenRig words', async () => {
    renderAt('/rig/r');
    act(() => FakeEventSource.instances[0].emit('snapshot', snapshot({ seats: [seatView('a')] })));
    await userEvent.setup().click(screen.getByRole('link', { name: 'Agents' }));
    expect(await screen.findByRole('table')).toBeInTheDocument();
    expect(screen.queryByTestId('graph-world')).toBeNull();
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('runtime reasons stay verbatim as raw evidence; UI text is still scanned', async () => {
    const reason = "host local reconnected without this seat's process";
    vi.mocked(api.latestSeatEvents).mockResolvedValue([
      ev(1, { type: 'seat-lifecycle-changed', lifecycle: 'attention', reason, provenance: 'observed' } as never),
      ev(2, { type: 'tenure-started', tenure: 1, cause: 'launch', driver: 'seat-driver' } as never),
    ]);
    renderAt('/rig/r/agent/dev.a%40r');
    act(() =>
      FakeEventSource.instances[0].emit(
        'snapshot',
        snapshot({ seats: [seatView('a', { lifecycle: 'attention', lifecycle_reason: reason }), seatView('b')] }),
      ),
    );
    await screen.findByText(/session #1 started/i);
    expect(document.body).toHaveTextContent("this seat's process");
    expect(document.body).toHaveTextContent('seat-driver');
    // The pane reason, the lifecycle notice and the driver are all marked; every place the
    // runtime text appears is inside raw evidence.
    expect(document.querySelectorAll('[data-raw-evidence]').length).toBeGreaterThanOrEqual(3);
    const holders = [...document.querySelectorAll('body *')].filter(
      (el) => [...el.childNodes].some((n) => n.nodeType === 3 && /this seat's process|seat-driver/.test(n.textContent ?? '')),
    );
    expect(holders.length).toBeGreaterThanOrEqual(3);
    for (const el of holders) expect(el.closest('[data-raw-evidence]')).not.toBeNull();
    expect(openRigWords(document.body)).toEqual([]);
    const extra = document.createElement('p');
    extra.textContent = 'This seat is stuck';
    document.body.appendChild(extra);
    expect(openRigWords(document.body)).toEqual(['seat']);
  });

  it('the scanner finds words and ignores Advanced and raw evidence', () => {
    document.body.innerHTML =
      '<p>Each seat has a pod</p><details data-advanced><summary>Advanced</summary>RigSpec YAML</details>' +
      '<table data-raw-evidence><tr><td>seat-lifecycle-changed</td></tr></table><button aria-label="Stop rig">x</button>';
    expect(openRigWords(document.body)).toEqual(['seat', 'pod', 'rig']);
  });
});
