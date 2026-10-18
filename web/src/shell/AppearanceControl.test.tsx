import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { AppearanceControl } from './AppearanceControl';
import { act } from 'react';
import { APPEARANCE_KEY, DEFAULT_APPEARANCE, FONTS, readAppearance, resolvedScheme, updateAppearance } from '../appearance';

beforeEach(() => localStorage.clear());
afterEach(() => { vi.restoreAllMocks(); document.documentElement.removeAttribute('style'); delete document.documentElement.dataset.surface; delete document.documentElement.dataset.look; delete document.documentElement.dataset.mode; });

describe('Appearance', () => {
  it('persists font, text size and surface changes across remounts', async () => {
    const user = userEvent.setup();
    const first = render(<AppearanceControl />);
    await user.click(screen.getByRole('button', { name: 'Appearance' }));
    await user.selectOptions(screen.getByLabelText('Interface font'), 'humanist');
    await user.click(screen.getByRole('button', { name: 'Larger text' }));
    await user.click(screen.getByRole('checkbox', { name: /Translucent surfaces/ }));
    expect(document.documentElement.style.getPropertyValue('--ui-size')).toBe('14px');
    expect(document.documentElement.style.getPropertyValue('--font-ui')).toBe(FONTS.humanist.value);
    expect(document.documentElement.dataset.surface).toBe('solid');
    first.unmount();
    render(<AppearanceControl />);
    await user.click(screen.getByRole('button', { name: 'Appearance' }));
    expect(screen.getByLabelText('Interface font')).toHaveValue('humanist');
    expect(screen.getByRole('slider', { name: /Text size/ })).toHaveValue('14');
    await user.click(screen.getByRole('button', { name: 'Reset defaults' }));
    expect(readAppearance()).toEqual(DEFAULT_APPEARANCE);
  });

  it('switches the look and, for Standard, the colour mode, and keeps both across remounts', async () => {
    const user = userEvent.setup();
    const first = render(<AppearanceControl />);
    await user.click(screen.getByRole('button', { name: 'Appearance' }));
    expect(screen.getByRole('button', { name: 'Coastal' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByRole('group', { name: 'Mode' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Standard' }));
    expect(document.documentElement.dataset.look).toBe('standard');
    expect(screen.getByRole('button', { name: 'System' })).toHaveAttribute('aria-pressed', 'true');
    // Coastal's translucency switch means nothing in the flat Standard look.
    expect(screen.queryByRole('checkbox', { name: /Translucent surfaces/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Dark' }));
    expect(document.documentElement.dataset.mode).toBe('dark');
    expect(readAppearance()).toMatchObject({ look: 'standard', mode: 'dark' });
    first.unmount();
    render(<AppearanceControl />);
    await user.click(screen.getByRole('button', { name: 'Appearance' }));
    expect(screen.getByRole('button', { name: 'Standard' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Dark' })).toHaveAttribute('aria-pressed', 'true');
    // A change from the command palette shows in the open dialog.
    act(() => updateAppearance({ mode: 'light' }));
    expect(screen.getByRole('button', { name: 'Light' })).toHaveAttribute('aria-pressed', 'true');
    expect(document.documentElement.dataset.mode).toBe('light');
    await user.click(screen.getByRole('button', { name: 'Reset defaults' }));
    expect(document.documentElement.dataset.look).toBe('coastal');
  });

  it('resolves the colour scheme: Coastal is dark, Standard follows its mode', () => {
    expect(resolvedScheme({ look: 'coastal', mode: 'light' })).toBe('dark');
    expect(resolvedScheme({ look: 'standard', mode: 'light' })).toBe('light');
    expect(resolvedScheme({ look: 'standard', mode: 'dark' })).toBe('dark');
  });

  it('contains keyboard focus, suppresses app shortcuts, and returns focus on Escape', async () => {
    const user = userEvent.setup();
    render(<AppearanceControl />);
    await user.click(screen.getByRole('button', { name: 'Appearance' }));
    expect(screen.getByRole('button', { name: 'Close appearance' })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Done' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Close appearance' })).toHaveFocus();
    const handler = vi.fn();
    window.addEventListener('keydown', handler);
    await user.keyboard('n');
    expect(handler).not.toHaveBeenCalled();
    window.removeEventListener('keydown', handler);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Appearance' })).toHaveFocus();
  });

  it('rejects malformed or out-of-range saved preferences', () => {
    for (const raw of ['{', 'null', '42', '{"font":"__proto__","size":99,"glass":"true"}', '{"look":"neon","mode":"dusk"}']) {
      localStorage.setItem(APPEARANCE_KEY, raw);
      expect(readAppearance()).toEqual(DEFAULT_APPEARANCE);
    }
  });

  it('still applies settings when browser storage is unavailable', async () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    const user = userEvent.setup();
    render(<AppearanceControl />);
    await user.click(screen.getByRole('button', { name: 'Appearance' }));
    await user.click(screen.getByRole('button', { name: 'Smaller text' }));
    expect(document.documentElement.style.getPropertyValue('--ui-size')).toBe('12px');
    expect(screen.getByRole('button', { name: 'Smaller text' })).toBeDisabled();
  });
});
