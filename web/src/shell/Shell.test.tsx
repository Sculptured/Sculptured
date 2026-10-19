import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { resetSeenCache } from '../seen';
import { FakeEventSource } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { forgetViewports } from './LiveGraph';
import { renderApp, snapQ, twoTeams, viewQ, where } from './shellHarness';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return {
    ...m,
    api: { ...m.api, latestSeatEvents: vi.fn(), teamRecent: vi.fn(), listRigs: vi.fn(), projectVisibility: vi.fn(), setProjectVisibility: vi.fn(), deleteProjectForever: vi.fn() },
  };
});

const topBar = () => document.querySelector<HTMLElement>('.topbar')!;

beforeEach(() => {
  FakeEventSource.reset();
  forgetViewports();
  localStorage.clear();
  resetSeenCache();
  vi.mocked(api.latestSeatEvents).mockReset().mockResolvedValue([]);
  vi.mocked(api.teamRecent).mockReset().mockResolvedValue({});
  vi.mocked(api.listRigs).mockReset().mockResolvedValue([]);
  vi.mocked(api.projectVisibility).mockReset().mockResolvedValue([]);
  vi.mocked(api.setProjectVisibility).mockReset().mockImplementation(async (id, status) => ({ id, status }));
});

describe('old addresses go to the page that does their job', () => {
  it.each([
    ['/team/t/structure', '/p/t/board?mode=design'],
    ['/team/t', '/p/t/board'],
    ['/team/t/live', '/p/t/board'],
    ['/rig/t', '/p/t/board'],
    ['/rig/t?view=history', '/p/t/results/history'],
    ['/team/t/progress', '/p/t/results'],
    ['/team/t/git', '/p/t/git'],
    ['/rig/t/agent/team.builder@t', '/p/t/agents/team.builder'],
    ['/rig/t/agent/team.builder@t/conversation', '/p/t/agents/team.builder/conversation'],
    ['/rig/t/agent/team.builder@t/visual', '/p/t/agents/team.builder/activity'],
    ['/rig/t/agent/team.builder@t/events', '/p/t/agents/team.builder/activity?view=events'],
    ['/rig/t/seat/team.builder@t', '/p/t/agents/team.builder'],
    ['/p/t', '/p/t/board'],
    ['/library', '/library/groups'],
    ['/no/such/page', '/'],
  ])('%s → %s', async (from, to) => {
    renderApp(from);
    await waitFor(() => expect(where()).toBe(to));
  });
});

describe('the frame: one top bar, then a project tab row, then the page', () => {
  it('the top bar holds home, the project switcher, Library, Account, Docs, Ctrl K and the app menu; no sidebar', async () => {
    await twoTeams('/rig/r');
    const bar = topBar();
    expect(within(bar).getByRole('link', { name: 'Sculptured: all projects' })).toHaveAttribute('href', '/');
    const nav = within(bar).getByRole('navigation', { name: 'Go to' });
    expect(within(nav).getByRole('link', { name: 'Library' })).toHaveAttribute('href', '/library/groups');
    expect(within(nav).getByRole('link', { name: /Account/ })).toHaveAttribute('href', '/account');
    const docs = within(nav).getByRole('link', { name: /Docs/ });
    expect(docs).toHaveAttribute('href', 'https://docs.usesculptured.com');
    expect(docs).toHaveAttribute('target', '_blank');
    expect(within(bar).getByRole('button', { name: /Search or run a command/ })).toBeInTheDocument();
    expect(screen.queryByRole('navigation', { name: 'Agents' })).toBeNull();
    // The switcher: the open project, every project with its status, New project and All projects.
    const user = userEvent.setup();
    await user.click(within(bar).getByLabelText(/^Project: r\. Switch project/));
    const list = within(bar).getByRole('list', { name: 'Projects' });
    expect(within(list).getAllByRole('link').map((l) => l.textContent)).toEqual(['qRunning · none working', 'rRunning · none working']);
    expect(within(bar).getByRole('link', { name: /New project/ })).toHaveAttribute('href', '/new');
    await user.click(within(list).getByRole('link', { name: /^q/ }));
    expect(where()).toBe('/p/q/board');
    // The app menu: appearance and keys, Community.
    await user.click(within(bar).getByLabelText('App menu'));
    for (const name of ['Appearance…', 'Keyboard shortcuts']) expect(within(bar).getByRole('button', { name })).toBeInTheDocument();
    expect(within(bar).getByRole('link', { name: 'Community' })).toHaveAttribute('href', '/community');
    // Phones hide the bar's links, so the menu has Docs too.
    expect(within(bar).getAllByRole('link', { name: /Docs/ })).toHaveLength(2);
  });

  it('every project page has Board · Agents · Questions · Results · Settings, a status in shape and words, and one primary action', async () => {
    await twoTeams('/rig/r');
    const tabs = screen.getByRole('navigation', { name: 'Project' });
    expect(within(tabs).getAllByRole('link').map((l) => l.textContent)).toEqual(['Board', 'Agents', 'Questions', 'Results', 'Settings']);
    expect(within(tabs).getByRole('link', { name: 'Board' })).toHaveAttribute('aria-current', 'page');
    const status = document.querySelector('.project-bar .pstatus')!;
    expect(status).toHaveTextContent('Running · none working');
    expect(status.querySelector('svg.icon')).not.toBeNull();
    expect(screen.getAllByRole('button', { name: /^(Start team|Stop team…)$/ })).toHaveLength(1);
    const user = userEvent.setup();
    for (const [tab, to] of [['Agents', '/p/r/agents'], ['Results', '/p/r/results'], ['Settings', '/p/r/settings']]) {
      await user.click(within(tabs).getByRole('link', { name: tab }));
      expect(where()).toBe(to);
      expect(within(tabs).getByRole('link', { name: tab })).toHaveAttribute('aria-current', 'page');
    }
  });

  it('the Library and Account pages sit in the same frame', async () => {
    await twoTeams('/library/groups');
    expect(screen.getByRole('heading', { name: 'Private Library' })).toBeVisible();
    const nav = within(topBar()).getByRole('navigation', { name: 'Go to' });
    expect(within(nav).getByRole('link', { name: 'Library' })).toHaveClass('on');
    await userEvent.setup().click(within(nav).getByRole('link', { name: /Account/ }));
    expect(where()).toBe('/account');
    expect(screen.getByRole('heading', { name: 'Your account' })).toBeInTheDocument();
  });
});

describe('Projects home', () => {
  it('lists every project with its status, agents and last activity; a name opens its board', async () => {
    const q = snapQ({ seats: [viewQ('x'), viewQ('y', { lifecycle: 'attention', lifecycle_reason: 'asks a question' })] });
    await twoTeams('/', undefined, q);
    expect(screen.getByRole('heading', { level: 1, name: 'Projects' })).toBeInTheDocument();
    const rows = within(screen.getByRole('table')).getAllByRole('row').slice(1);
    // The one that needs you first, its status a shape and words.
    expect(rows[0]).toHaveTextContent('qNeeds you · 1 of 2');
    expect(rows[0].querySelector('.pstatus svg.icon')).not.toBeNull();
    expect(rows[1]).toHaveTextContent('rRunning · none working');
    // One primary action on the page: New project.
    expect(screen.getAllByRole('link', { name: 'New project' }).filter((l) => l.classList.contains('btn-primary'))).toHaveLength(1);
    await userEvent.setup().click(within(rows[1]).getByRole('link', { name: 'r' }));
    expect(where()).toBe('/p/r/board');
    expect(openRigWords(document.querySelector<HTMLElement>('.app')!)).toEqual([]);
  });

  it('hides and shows projects; a running one says to stop it before the Trash', async () => {
    vi.mocked(api.deleteProjectForever).mockResolvedValue({ deleted: 'r' });
    await twoTeams('/');
    const user = userEvent.setup();
    const row = () => screen.getByRole('link', { name: 'r' }).closest('tr')!;
    await user.click(screen.getByLabelText('Manage r'));
    await user.click(within(row()).getByRole('button', { name: 'Hide' }));
    await screen.findByText('Hidden projects (1)');
    expect(screen.queryByRole('link', { name: 'r' })).toBeNull();
    await user.click(screen.getByText('Hidden projects (1)'));
    await user.click(screen.getByRole('button', { name: 'Show' }));
    expect(await screen.findByRole('link', { name: 'r' })).toBeInTheDocument();
    expect(api.setProjectVisibility).toHaveBeenLastCalledWith('r', 'visible');
    await user.click(screen.getByLabelText('Manage r'));
    expect(within(row()).getByRole('button', { name: 'Move to Trash' })).toBeDisabled();
    expect(within(row()).getByText('Stop the team before moving it to the Trash.')).toBeInTheDocument();
  });

  it('a project in the Trash can be restored, or deleted forever after an explicit second confirmation', async () => {
    vi.mocked(api.projectVisibility).mockResolvedValue([{ id: 'r', status: 'trashed' }]);
    vi.mocked(api.deleteProjectForever).mockResolvedValue({ deleted: 'r' });
    await twoTeams('/');
    const user = userEvent.setup();
    await user.click(await screen.findByText('Trash (1)'));
    await user.click(screen.getByRole('button', { name: 'Delete r forever' }));
    expect(api.deleteProjectForever).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent("Delete r forever? Its design, runs and history can't be restored.");
    await user.click(screen.getByRole('button', { name: 'Yes, delete forever' }));
    expect(api.deleteProjectForever).toHaveBeenCalledWith('r');
    await waitFor(() => expect(screen.queryByText('Trash (1)')).toBeNull());
  });
});
