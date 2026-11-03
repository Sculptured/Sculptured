import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PopOutPane } from './PopOutPane';
import { defaults, FakeChannel, fakeMatchMedia, registry } from './testkit';
import { WorkspacePage } from './WorkspacePage';

const paneRoute = (id: string) => `/p/t/control/pane/${id}`;
const page = (key?: string) => <WorkspacePage key={key} page="control" registry={registry} ctx={{}} defaults={defaults} paneRoute={paneRoute} header={<p>Header</p>} />;
const layoutBar = () => screen.getByRole('tablist', { name: 'Layouts' });
const layoutNames = () => within(layoutBar()).getAllByRole('tab').map((t) => t.textContent);
const activeLayout = () => within(layoutBar()).getByRole('tab', { selected: true }).textContent;
const paneTab = (name: string, scope: HTMLElement = document.body) => within(scope).getByRole('tab', { name: new RegExp(name) });
const layoutMenu = async (user: ReturnType<typeof userEvent.setup>, item: string) => {
  await user.click(screen.getByRole('button', { name: 'Layout options' }));
  await user.click(screen.getByRole('menuitem', { name: new RegExp(`^${item}`) }));
};

beforeEach(() => {
  localStorage.clear();
  FakeChannel.open.clear();
  vi.stubGlobal('BroadcastChannel', FakeChannel);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('WorkspacePage', () => {
  it('layout_tabs_add_rename_duplicate_delete_reset', async () => {
    const user = userEvent.setup();
    render(page());
    expect(screen.getByText('Header')).toBeInTheDocument();
    expect(layoutNames()).toEqual(['Main']);
    await user.click(screen.getByRole('button', { name: 'Add layout' }));
    expect(layoutNames()).toEqual(['Main', 'Main 2']);
    expect(activeLayout()).toBe('Main 2');
    expect(screen.queryAllByTestId(/^body-/)).toHaveLength(2);

    await user.dblClick(within(layoutBar()).getByRole('tab', { name: 'Main 2' }));
    const field = screen.getByRole('textbox', { name: 'Layout name' });
    await user.clear(field);
    await user.type(field, 'Watch{Enter}');
    expect(layoutNames()).toEqual(['Main', 'Watch']);
    expect(within(layoutBar()).getByRole('tab', { name: 'Watch' })).toHaveFocus();

    await layoutMenu(user, 'Duplicate');
    expect(layoutNames()).toEqual(['Main', 'Watch', 'Watch copy']);
    expect(activeLayout()).toBe('Watch copy');
    await layoutMenu(user, 'Delete');
    expect(layoutNames()).toEqual(['Main', 'Watch']);
    expect(activeLayout()).toBe('Watch');
    await layoutMenu(user, 'Rename');
    await user.type(screen.getByRole('textbox', { name: 'Layout name' }), 'Ops{Escape}');
    expect(layoutNames()).toEqual(['Main', 'Watch']);

    await user.click(within(layoutBar()).getByRole('tab', { name: 'Main' }));
    await user.click(screen.getByRole('button', { name: 'Close Overview' }));
    expect(screen.queryByTestId('body-b1')).toBeNull();
    await user.dblClick(within(layoutBar()).getByRole('tab', { name: 'Main' }));
    await user.type(screen.getByRole('textbox', { name: 'Layout name' }), 'Mine{Enter}');
    await layoutMenu(user, 'Reset to default');
    expect(layoutNames()).toEqual(['Main', 'Watch']);
    // A reset copies the default with fresh pane ids, so no two layouts share one.
    expect(paneTab('Overview')).toBeInTheDocument();

    await layoutMenu(user, 'Delete');
    expect(layoutNames()).toEqual(['Watch']);
    await user.click(screen.getByRole('button', { name: 'Layout options' }));
    const del = screen.getByRole('menuitem', { name: /^Delete/ });
    expect(del).toBeDisabled();
    expect(del).toHaveTextContent('the only layout');
  });

  it('add_pane_marks_shown_and_focuses', async () => {
    const user = userEvent.setup();
    render(page());
    await user.click(screen.getByRole('button', { name: '+ Add pane' }));
    const items = screen.getAllByRole('menuitem').map((i) => i.textContent);
    expect(items).toEqual(['◫ Overview · shown', '◉ Agent detail', '≡ Status feed']);
    await user.click(screen.getByRole('menuitem', { name: /Overview/ }));
    expect(screen.getAllByRole('tab', { name: /Overview/ })).toHaveLength(1);
    expect(paneTab('Overview')).toHaveFocus();

    await user.click(paneTab('Agent detail'));
    await user.click(screen.getByRole('button', { name: '+ Add pane' }));
    await user.click(screen.getByRole('menuitem', { name: /Status feed/ }));
    const feed = paneTab('Status feed');
    expect(feed).toHaveFocus();
    expect(feed.closest('[data-stack]')).toBe(paneTab('Agent detail').closest('[data-stack]'));

    await user.click(screen.getByRole('button', { name: '+ Add pane' }));
    await user.click(screen.getByRole('menuitem', { name: /Agent detail/ }));
    expect(screen.getAllByRole('tab', { name: /Agent detail/ })).toHaveLength(2);
  });

  it('layouts_persist_across_remount', async () => {
    const user = userEvent.setup();
    const first = render(page());
    await user.click(screen.getByRole('button', { name: 'Add layout' }));
    await user.click(screen.getByRole('button', { name: 'Close Agent detail' }));
    first.unmount();
    render(page());
    expect(layoutNames()).toEqual(['Main', 'Main 2']);
    expect(activeLayout()).toBe('Main 2');
    expect(screen.queryByRole('tab', { name: /Agent detail/ })).toBeNull();
    expect(paneTab('Overview')).toBeInTheDocument();
  });

  it('look_setting_sets_data_look', async () => {
    const user = userEvent.setup();
    const first = render(page());
    const root = () => document.querySelector<HTMLElement>('.workspace')!;
    expect(root()).toHaveAttribute('data-look', 'mission');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Look' }), 'Follow app look');
    expect(root()).not.toHaveAttribute('data-look');
    first.unmount();
    render(page());
    expect(root()).not.toHaveAttribute('data-look');
    expect(screen.getByRole('combobox', { name: 'Look' })).toHaveValue('app');
  });

  it('mobile_shows_one_pane_with_switcher', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('matchMedia', fakeMatchMedia(true));
    render(page());
    const switcher = screen.getByRole('combobox', { name: 'Pane' });
    expect(within(switcher).getAllByRole('option').map((o) => o.textContent)).toEqual(['Overview', 'Agent detail']);
    expect(screen.getByTestId('body-b1')).toBeInTheDocument();
    expect(screen.queryByTestId('body-a1')).toBeNull();
    expect(screen.queryByRole('separator')).toBeNull();
    await user.selectOptions(switcher, 'Agent detail');
    expect(screen.getByTestId('body-a1')).toBeInTheDocument();
    expect(screen.queryByTestId('body-b1')).toBeNull();
    await user.click(screen.getByRole('button', { name: '+ Add pane' }));
    await user.click(screen.getByRole('menuitem', { name: /Status feed/ }));
    expect(screen.getByRole('combobox', { name: 'Pane' })).toHaveDisplayValue('Status feed');
  });

  it('selection_shared_between_two_workspaces', async () => {
    const user = userEvent.setup();
    const { container } = render(<>{page('one')}{page('two')}</>);
    const [one, two] = [...container.querySelectorAll<HTMLElement>('.workspace')];
    await user.click(within(one).getByRole('button', { name: 'Select A' }));
    expect(within(one).getByTestId('sel-a1')).toHaveTextContent('A');
    expect(within(two).getByTestId('sel-a1')).toHaveTextContent('A');

    await user.click(within(two).getByRole('button', { name: 'Agent detail options' }));
    await user.click(within(two).getByRole('menuitem', { name: 'Settings' }));
    await user.click(within(two).getByRole('checkbox', { name: 'Follow selection' }));
    await user.click(within(two).getByRole('button', { name: 'Done' }));
    expect(within(two).getByRole('button', { name: 'Agent detail options' })).toHaveFocus();
    await user.click(within(one).getByRole('button', { name: 'Select B' }));
    expect(within(one).getByTestId('sel-a1')).toHaveTextContent('B');
    expect(within(two).getByTestId('sel-a1')).toHaveTextContent('A');
  });

  it('popout_moves_and_bring_back', async () => {
    const user = userEvent.setup();
    const open = vi.spyOn(window, 'open').mockReturnValueOnce(null);
    render(page());
    const pop = async () => {
      await user.click(screen.getByRole('button', { name: 'Overview options' }));
      await user.click(screen.getByRole('menuitem', { name: 'Pop out' }));
    };
    await pop();
    expect(screen.getByText(/blocked the new window/)).toBeInTheDocument();
    expect(screen.getByTestId('body-b1')).toBeInTheDocument();

    const win = { closed: false, focus: vi.fn(), close: vi.fn() };
    open.mockReturnValue(win as unknown as Window);
    await pop();
    expect(open).toHaveBeenLastCalledWith('/p/t/control/pane/b1', 'sculptured-pane-b1', expect.any(String));
    expect(screen.queryByTestId('body-b1')).toBeNull();
    expect(screen.getByText('Shown in another window.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Overview options' })).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'Bring back' }));
    expect(win.close).toHaveBeenCalled();
    expect(screen.getByTestId('body-b1')).toBeInTheDocument();
    expect(screen.queryByText('Shown in another window.')).toBeNull();

    await pop();
    const popped = render(<PopOutPane page="control" registry={registry} ctx={{}} instance="b1" />);
    expect(within(popped.container).getByTestId('body-b1')).toBeInTheDocument();
    expect(within(popped.container).getByRole('heading', { name: 'Overview' })).toBeInTheDocument();
    act(() => void window.dispatchEvent(new Event('pagehide')));
    expect(screen.queryByText('Shown in another window.')).toBeNull();
    expect(screen.getAllByTestId('body-b1')).toHaveLength(2);
  });
});
