import { act, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { forgetViewports, holdViewport, usePanZoom, type Bounds, type Viewport } from './viewport';

// A Run finished panel docking beside the board narrows it: the cards must stay where they are.
let resize: (() => void) | null = null;
class FakeResizeObserver {
  constructor(cb: () => void) { resize = cb; }
  observe() {}
  disconnect() {}
}

const size = { w: 1200, h: 800 };
function Board({ bounds, seen }: { bounds: Bounds; seen: (v: Viewport) => void }) {
  const { el, vp } = usePanZoom('team:live:all', bounds);
  seen(vp);
  return <div ref={(n) => {
    el.current = n;
    if (n) {
      Object.defineProperty(n, 'clientWidth', { configurable: true, get: () => size.w });
      Object.defineProperty(n, 'clientHeight', { configurable: true, get: () => size.h });
    }
  }} />;
}

beforeEach(() => { forgetViewports(); vi.stubGlobal('ResizeObserver', FakeResizeObserver); size.w = 1200; size.h = 800; });
afterEach(() => { vi.unstubAllGlobals(); resize = null; });

it('keeps the view when a panel docks beside the board and the drawing still shows', () => {
  let vp: Viewport = { x: 0, y: 0, k: 1 };
  const bounds = { x0: 0, y0: 0, x1: 300, y1: 200 };
  render(<Board bounds={bounds} seen={(v) => { vp = v; }} />);
  act(() => resize?.());
  const before = vp;
  // The panel docks: the board is 380px narrower, and asked to hold its view first.
  holdViewport('team:live:all');
  size.w = 820;
  act(() => resize?.());
  expect(vp).toEqual(before);
  // A later resize of the window fits again, as before.
  size.w = 1000;
  act(() => resize?.());
  expect(vp).not.toEqual(before);
});

it('fits again when the narrower board would cut the drawing off, unless the user moved the view', () => {
  let vp: Viewport = { x: 0, y: 0, k: 1 };
  const bounds = { x0: 0, y0: 0, x1: 1000, y1: 200 };
  render(<Board bounds={bounds} seen={(v) => { vp = v; }} />);
  act(() => resize?.());
  const before = vp;
  holdViewport('team:live:all');
  size.w = 700;
  act(() => resize?.());
  expect(vp).not.toEqual(before);
  expect(vp.x + bounds.x1 * vp.k).toBeLessThanOrEqual(700);
});
