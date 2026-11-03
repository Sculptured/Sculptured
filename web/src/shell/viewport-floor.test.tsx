import { act, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { FitFloor, forgetViewports, usePanZoom, ViewportScope, type Bounds, type Viewport } from './viewport';

// A Control pane fits its board at a readable zoom; Fit still shows the whole board.
let resize: (() => void) | null = null;
class FakeResizeObserver {
  constructor(cb: () => void) { resize = cb; }
  observe() {}
  disconnect() {}
}

const size = { w: 600, h: 400 };
const BIG: Bounds = { x0: 0, y0: 0, x1: 2400, y1: 1200 };
type Api = ReturnType<typeof usePanZoom>;

function Board({ seen }: { seen: (a: Api) => void }) {
  const api = usePanZoom('team:live:all', BIG);
  seen(api);
  return <div ref={(n) => {
    api.el.current = n;
    if (n) {
      Object.defineProperty(n, 'clientWidth', { configurable: true, get: () => size.w });
      Object.defineProperty(n, 'clientHeight', { configurable: true, get: () => size.h });
    }
  }} />;
}

const inside = (v: Viewport) => v.x >= 0 && v.y >= 0 && v.x + BIG.x1 * v.k <= size.w && v.y + BIG.y1 * v.k <= size.h;

beforeEach(() => { forgetViewports(); vi.stubGlobal('ResizeObserver', FakeResizeObserver); size.w = 600; size.h = 400; });
afterEach(() => { vi.unstubAllGlobals(); resize = null; });

it('fits at the floor, but Fit shows the whole board, also after a resize, until the user zooms', () => {
  let api!: Api;
  render(<FitFloor.Provider value={0.75}><Board seen={(a) => { api = a; }} /></FitFloor.Provider>);
  act(() => resize?.());
  expect(api.vp.k).toBe(0.75);
  act(() => api.fit());
  expect(api.vp.k).toBeLessThan(0.75);
  expect(inside(api.vp)).toBe(true);
  size.w = 500;
  act(() => resize?.());
  expect(inside(api.vp)).toBe(true);
  // Zooming ends it: the view then stays as the user left it.
  act(() => api.zoomAt(1.1));
  const k = api.vp.k;
  act(() => resize?.());
  expect(api.vp.k).toBe(k);
});

it('a scoped board keeps its own remembered view', () => {
  let a!: Api;
  let b!: Api;
  render(<><ViewportScope.Provider value="control:"><Board seen={(x) => { a = x; }} /></ViewportScope.Provider></>);
  act(() => a.zoomAt(2));
  render(<Board seen={(x) => { b = x; }} />);
  expect(b.vp.k).not.toBe(a.vp.k);
});
