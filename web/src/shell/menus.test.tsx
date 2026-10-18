import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { useDetailsMenus } from './menus';

function Header() {
  useDetailsMenus();
  return (
    <div>
      <details className="more-menu"><summary>More</summary><div className="more-items"><button type="button">Token use</button></div></details>
      <details className="team-status"><summary>Status</summary><div className="team-info">2 of 2 agents running</div></details>
      <button type="button">Elsewhere</button>
    </div>
  );
}

const menu = (name: string) => screen.getByText(name, { selector: 'summary' }).closest('details')!;

describe('header menus', () => {
  it('only one is open at a time', async () => {
    render(<Header />);
    const user = userEvent.setup();
    await user.click(screen.getByText('More'));
    expect(menu('More')).toHaveAttribute('open');
    await user.click(screen.getByText('Status'));
    await waitFor(() => expect(menu('More')).not.toHaveAttribute('open'));
    expect(menu('Status')).toHaveAttribute('open');
  });

  it('Esc closes the open menu and returns focus to its button', async () => {
    render(<Header />);
    const user = userEvent.setup();
    await user.click(screen.getByText('More'));
    await user.click(screen.getByRole('button', { name: 'Token use' }));
    await user.keyboard('{Escape}');
    expect(menu('More')).not.toHaveAttribute('open');
    expect(screen.getByText('More')).toHaveFocus();
  });

  it('a click outside closes it; a click inside does not', async () => {
    render(<Header />);
    const user = userEvent.setup();
    await user.click(screen.getByText('Status'));
    fireEvent.pointerDown(screen.getByText('2 of 2 agents running'));
    expect(menu('Status')).toHaveAttribute('open');
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Elsewhere' }));
    expect(menu('Status')).not.toHaveAttribute('open');
  });

  it('Esc with no menu open is left to the page', () => {
    render(<Header />);
    const e = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    document.body.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
  });
});
