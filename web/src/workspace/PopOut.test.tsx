import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import { PopOutPane } from './PopOutPane';
import { FakeChannel, page, registry } from './testkit';

const SLOT = 'Shown in another window.';
const BACK = 'This pane is back in the main window. You can close this window.';
const route = (id: string) => `/p/t/control/pane/${id}`;
let open: MockInstance<typeof window.open>;
let close: MockInstance<typeof window.close>;
let user: ReturnType<typeof userEvent.setup>;

/** A pop-out window for `instance`, mounted beside the main page. `reload` is what the browser does on reload. */
function popWindow(instance: string) {
  const view = render(<PopOutPane page="control" registry={registry} ctx={{}} instance={instance} />);
  const reload = () => { act(() => void window.dispatchEvent(new Event('pagehide'))); view.unmount(); return popWindow(instance); };
  return { ...view, scope: () => within(view.container), reload };
}

async function popOut(title: string, scope: HTMLElement = document.body) {
  await user.click(within(scope).getByRole('button', { name: `${title} options` }));
  await user.click(within(scope).getByRole('menuitem', { name: 'Pop out' }));
}

beforeEach(() => {
  localStorage.clear();
  FakeChannel.open.clear();
  vi.stubGlobal('BroadcastChannel', FakeChannel);
  user = userEvent.setup();
  open = vi.spyOn(window, 'open').mockReturnValue({ closed: false, focus: vi.fn() } as unknown as Window);
  close = vi.spyOn(window, 'close').mockImplementation(() => {});
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('pop out', () => {
  it('popout_moves_and_bring_back', async () => {
    open.mockReturnValueOnce(null);
    const main = render(page());
    await popOut('Overview');
    expect(screen.getByText(/blocked the new window/)).toBeInTheDocument();
    expect(screen.getByTestId('body-b1')).toBeInTheDocument();

    await popOut('Overview');
    expect(open).toHaveBeenLastCalledWith(route('b1'), 'sculptured-pane-b1', expect.any(String));
    expect(screen.queryByTestId('body-b1')).toBeNull();
    expect(screen.getByText(SLOT)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Overview options' })).toHaveFocus();
    await user.click(screen.getByRole('button', { name: 'Bring back' }));
    expect(screen.getByTestId('body-b1')).toBeInTheDocument();
    expect(screen.queryByText(SLOT)).toBeNull();

    await popOut('Overview');
    const win = popWindow('b1');
    expect(win.scope().getByTestId('body-b1')).toBeInTheDocument();
    expect(win.scope().getByRole('heading', { name: 'Overview' })).toBeInTheDocument();
    act(() => void window.dispatchEvent(new Event('pagehide')));
    expect(within(main.container).getByTestId('body-b1')).toBeInTheDocument();
    expect(screen.queryByText(SLOT)).toBeNull();
  });

  it('popout_remount_ends_in_popped_again', async () => {
    const main = render(page());
    await popOut('Overview');
    const again = popWindow('b1').reload();
    expect(within(main.container).getByText(SLOT)).toBeInTheDocument();
    expect(within(main.container).queryByTestId('body-b1')).toBeNull();
    expect(again.scope().getByTestId('body-b1')).toBeInTheDocument();
  });

  it('popped_layout_survives_remount_and_bring_back_closes_the_window', async () => {
    const first = render(page());
    await popOut('Overview');
    const win = popWindow('b1');
    first.unmount();
    const main = render(page());
    expect(within(main.container).getByText(SLOT)).toBeInTheDocument();
    await user.click(within(main.container).getByRole('button', { name: 'Bring back' }));
    expect(within(main.container).getByTestId('body-b1')).toBeInTheDocument();
    expect(close).toHaveBeenCalledTimes(1);
    expect(win.scope().getByText(BACK)).toBeInTheDocument();
  });

  it('show_that_window_without_a_handle_does_not_reload_it', async () => {
    const first = render(page());
    await popOut('Overview');
    first.unmount();
    render(page());
    const replace = vi.fn();
    open.mockReturnValue({ closed: false, focus: vi.fn(), location: { href: route('b1'), replace } } as unknown as Window);
    await user.click(screen.getByRole('button', { name: 'Show that window' }));
    expect(open).toHaveBeenLastCalledWith('', 'sculptured-pane-b1', expect.any(String));
    expect(replace).not.toHaveBeenCalled();
  });

  it('settings_edit_after_reload_applied', async () => {
    const main = render(page());
    const scope = main.container;
    await popOut('Agent detail', scope);
    const win = popWindow('a1').reload();
    await user.click(win.scope().getByRole('button', { name: 'Settings' }));
    await user.click(win.scope().getByRole('checkbox', { name: 'Follow selection' }));
    await user.click(within(scope).getByRole('button', { name: 'Bring back' }));
    await user.click(within(scope).getByRole('button', { name: 'Agent detail options' }));
    await user.click(within(scope).getByRole('menuitem', { name: 'Settings' }));
    expect(within(scope).getByRole('checkbox', { name: 'Follow selection' })).not.toBeChecked();
  });

  it('closing_the_slot_brings_the_pane_back', async () => {
    const main = render(page());
    await popOut('Overview');
    const win = popWindow('b1');
    await user.click(within(main.container).getByRole('button', { name: 'Close Overview' }));
    expect(within(main.container).getByTestId('body-b1')).toBeInTheDocument();
    expect(within(main.container).queryByText(SLOT)).toBeNull();
    expect(win.scope().getByText(BACK)).toBeInTheDocument();
  });

  it('ask_handshake_for_a_window_opened_later', async () => {
    const main = render(page());
    await user.click(within(main.container).getByRole('button', { name: 'Select A' }));
    const win = popWindow('a1');
    expect(win.scope().getByTestId('sel-a1')).toHaveTextContent('A');
  });

  it('without_broadcast_channel_works_in_one_window', async () => {
    vi.stubGlobal('BroadcastChannel', undefined);
    const { container } = render(<>{page('one')}{page('two')}</>);
    const [one, two] = [...container.querySelectorAll<HTMLElement>('.workspace')];
    await user.click(within(one).getByRole('button', { name: 'Select A' }));
    expect(within(one).getByTestId('sel-a1')).toHaveTextContent('A');
    expect(within(two).getByTestId('sel-a1')).toHaveTextContent('none');
    await popOut('Overview', one);
    expect(within(one).getByText(SLOT)).toBeInTheDocument();
    await user.click(within(one).getByRole('button', { name: 'Bring back' }));
    expect(within(one).getByTestId('body-b1')).toBeInTheDocument();
  });
});
