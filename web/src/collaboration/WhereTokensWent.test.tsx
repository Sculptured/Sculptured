import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { api, ApiError } from '../api';
import type { TokensReport } from '../tokensReport';
import golden from './fixtures/tokens-report.json';
import { WhereTokensWent } from './WhereTokensWent';

afterEach(() => vi.restoreAllMocks());

/** The MAIS golden report of the fixture run, with a plan window, its budget and a fan-out added. */
function report(): TokensReport {
  const r = structuredClone(golden) as unknown as TokensReport;
  r.plan = [{ provider: 'claude', window: 'five_hour', points: [[1, 18, 18], [2, 48, 48]], first: 18, highest: 48, added: { value: 30, provenance: 'derived' },
    points_per_million: { value: 1, provenance: 'derived' }, budget: [25.5, 30], events: [{ kind: 'stop', at_ms: 1792094040000, reason: 'Claude 5-hour: 30 points' }] }];
  r.timeline.fanouts = [{ at_ms: r.timeline.points[0].at_ms, agents: 5, from: 'dev8' }];
  return r;
}

const show = (start?: number) => render(<MemoryRouter><WhereTokensWent rig="c172" start={start} seats={['fuel.dev8@c172']} onClose={() => {}} /></MemoryRouter>);

it('leads with what stands out, then the plan window against its budget and every cause with its provenance', async () => {
  vi.spyOn(api, 'teamReport').mockResolvedValue({ start_seq: 7, at_ms: Date.now(), running: false, report: report(), stale: null });
  show();
  expect(await screen.findByText(/Your own CLI setup \(skills list, plugins, hooks\) was 34%/)).toBeInTheDocument();
  expect(screen.getByRole('img', { name: /Claude 5-hour window: 18% before the run, 48% at its highest, 30 points added; the budget wraps up at 25.5 and stops at 30 points/ })).toBeInTheDocument();
  expect(screen.getByText(/Stopped: Claude 5-hour: 30 points/)).toBeInTheDocument();
  expect(screen.getByRole('img', { name: /Tokens and agents working/ })).toBeInTheDocument();
  expect(screen.getByText('5 started from Developer 8')).toBeInTheDocument();
  const causes = screen.getByRole('region', { name: 'Starting context, re-read on every call' });
  expect(within(causes).getByText('Your own CLI setup (skills list, plugins, hooks)').closest('li')).toHaveTextContent('estimated');
});

it('links each agent to its page, and a subagent to its parent', async () => {
  vi.spyOn(api, 'teamReport').mockResolvedValue({ start_seq: 7, at_ms: Date.now(), running: true, report: report(), stale: null });
  show(7);
  const table = await screen.findByRole('table', { name: /Every agent/ });
  const links = within(table).getAllByRole('link', { name: 'Developer 8' });
  expect(links[0]).toHaveAttribute('href', '/p/c172/agents/fuel.dev8');
  expect(within(table).getByText('List files')).toBeInTheDocument();
  expect(within(table).getByText(/started by/)).toBeInTheDocument();
  expect(api.teamReport).toHaveBeenCalledWith('c172', 7);
  expect(screen.getByText(/refreshed every minute/)).toBeInTheDocument();
});

it('says plainly when the runner cannot make a report, and when it shows an older one', async () => {
  vi.spyOn(api, 'teamReport').mockRejectedValue(new ApiError(409, 'runner_without_reports', "This computer's Sculptured runner is older than reports of where tokens went. Reinstall it from Computers."));
  const { unmount } = show();
  expect(await screen.findByRole('alert')).toHaveTextContent('Reinstall it from Computers');
  unmount();
  vi.spyOn(api, 'teamReport').mockResolvedValue({ start_seq: 7, at_ms: 1, running: true, report: report(), stale: 'The computer is not connected.' });
  show();
  expect(await screen.findByText(/a newer one could not be made: The computer is not connected/)).toBeInTheDocument();
});

it('keeps keyboard focus where the person put it when the board re-renders', async () => {
  vi.spyOn(api, 'teamReport').mockResolvedValue({ start_seq: 7, at_ms: Date.now(), running: true, report: report(), stale: null });
  const view = (onClose: () => void) => <MemoryRouter><WhereTokensWent rig="c172" seats={['fuel.dev8@c172']} onClose={onClose} /></MemoryRouter>;
  const { rerender } = render(view(() => {}));
  expect(screen.getByRole('heading', { name: 'Where tokens went' })).toHaveFocus();
  const link = (await screen.findAllByRole('link', { name: 'Developer 8' }))[0];
  link.focus();
  rerender(view(() => {}));
  expect(link).toHaveFocus();
});
