import { act, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { ConnectingPanel, STALL_MS } from './ConnectingPanel';

afterEach(() => vi.useRealTimers());

it('says it is connecting at first, then plainly that live updates are not arriving, with a way out', () => {
  vi.useFakeTimers();
  render(<MemoryRouter><ConnectingPanel rig="shop" /></MemoryRouter>);
  expect(screen.getByRole('status')).toHaveTextContent('Connecting to shop…');
  act(() => { vi.advanceTimersByTime(STALL_MS + 1); });
  expect(screen.queryByText(/Connecting to shop/)).toBeNull();
  expect(screen.getByRole('alert')).toHaveTextContent("Live updates for shop aren't arriving");
  expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Open the Design board' })).toHaveAttribute('href', '/p/shop/board?mode=design');
});
