import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { HostedProvider } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';
import { SetupGate } from './SetupGate';
import { SetupPage } from './SetupPage';
import { Tour, TOUR_TIPS } from './Tour';

afterEach(() => vi.restoreAllMocks());
const reply = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }));

function Board() {
  return (
    <div>
      <button type="button" data-agent="builder">Builder card</button>
      <div data-tour="agent-folder">Folder field</div>
      <button type="button" data-tour="start">Start team</button>
      <span data-tour="board-mode">Design | Run</span>
      <a href="/q" data-tour="questions">Questions</a>
    </div>
  );
}

it('steps through the tips with Next and Back, each pinned to its real element', async () => {
  const onEnd = vi.fn();
  render(<><Board /><Tour onEnd={onEnd} /></>);
  const user = userEvent.setup();
  const tip = screen.getByRole('dialog', { name: TOUR_TIPS[0].title });
  expect(tip).toHaveTextContent(`1 of ${TOUR_TIPS.length}`);
  expect(screen.getByText('Builder card')).toHaveAttribute('data-tour-on');
  expect(screen.getByRole('button', { name: 'Next' })).toHaveFocus();
  expect(screen.getByRole('button', { name: 'Back' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'Next' }));
  expect(screen.getByRole('dialog', { name: TOUR_TIPS[1].title })).toHaveTextContent(`2 of ${TOUR_TIPS.length}`);
  expect(screen.getByText('Builder card')).not.toHaveAttribute('data-tour-on');
  expect(screen.getByText('Folder field')).toHaveAttribute('data-tour-on');
  await user.click(screen.getByRole('button', { name: 'Back' }));
  expect(screen.getByRole('dialog', { name: TOUR_TIPS[0].title })).toBeInTheDocument();
  for (let i = 0; i < TOUR_TIPS.length - 1; i++) await user.click(screen.getByRole('button', { name: 'Next' }));
  await user.click(screen.getByRole('button', { name: 'Finish' }));
  expect(onEnd).toHaveBeenCalledWith(true);
});

it('Esc and Skip end the tour early', async () => {
  const onEnd = vi.fn();
  const { unmount } = render(<><Board /><Tour onEnd={onEnd} /></>);
  const user = userEvent.setup();
  await user.keyboard('{Escape}');
  expect(onEnd).toHaveBeenLastCalledWith(false);
  unmount();
  render(<><Board /><Tour onEnd={onEnd} /></>);
  await user.click(screen.getByRole('button', { name: 'Skip tour' }));
  expect(onEnd).toHaveBeenCalledTimes(2);
});

it('a tip whose element is not on this page says where to find it', () => {
  render(<Tour onEnd={() => {}} />);
  expect(screen.getByRole('dialog')).toHaveTextContent(/not on this page/);
});

it('First project creates a Claude team, opens its board with the tour, and the tour marks setup', async () => {
  const puts: { done: string[]; skipped: string[] }[] = [];
  const created: unknown[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation((url, init) => {
    const u = String(url);
    const method = init?.method ?? 'GET';
    if (u.endsWith('/api/setup') && method === 'PUT') { const b = JSON.parse(String(init!.body)); puts.push(b); return reply(b); }
    if (u.endsWith('/api/setup')) return reply({ done: ['welcome', 'computer', 'models', 'computers'], skipped: [] });
    if (u.endsWith('/api/teams') && method === 'POST') { created.push(JSON.parse(String(init!.body))); return reply({ draft: { id: 'first-1', name: 'First project', agents: [] } }); }
    return reply({}, 404);
  });
  const session: HostedSession = { kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x.com', role: 'member' }, csrf: 'c' };
  render(
    <MemoryRouter initialEntries={['/setup']}>
      <HostedProvider value={session}>
        <SetupGate>
          <Routes>
            <Route path="/setup" element={<SetupPage />} />
            <Route path="/p/:team/board" element={<Board />} />
            <Route path="/" element={<p>projects page</p>} />
          </Routes>
        </SetupGate>
      </HostedProvider>
    </MemoryRouter>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Create the first project' }));
  await waitFor(() => expect(created[0]).toMatchObject({ template: 'builder-reviewer', practice: false }));
  expect(await screen.findByRole('dialog', { name: TOUR_TIPS[0].title })).toBeInTheDocument();
  expect(screen.getByText('Builder card')).toBeInTheDocument();
  for (let i = 0; i < TOUR_TIPS.length - 1; i++) await user.click(screen.getByRole('button', { name: 'Next' }));
  await user.click(screen.getByRole('button', { name: 'Finish' }));
  await waitFor(() => expect(puts.at(-1)?.done).toContain('tour'));
  expect(await screen.findByText('projects page')).toBeInTheDocument();
});
