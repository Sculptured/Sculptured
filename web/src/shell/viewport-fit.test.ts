import {expect,it} from 'vitest';
import {fitViewport} from './viewport';
it('fits a wide ten-child tree without clipping either side',()=>{
 const b={x0:0,y0:0,x1:3000,y1:1200};const v=fitViewport(b,700,480);
 expect(v.x).toBeGreaterThanOrEqual(0);expect(v.y).toBeGreaterThanOrEqual(0);
 expect(v.x+b.x1*v.k).toBeLessThanOrEqual(700);expect(v.y+b.y1*v.k).toBeLessThanOrEqual(480);
});

it('with a floor, fits no smaller than it and starts at the top-left corner', () => {
  const b = { x0: 0, y0: 0, x1: 3000, y1: 1200 };
  const v = fitViewport(b, 700, 480, 1.15, 0.7);
  expect(v.k).toBe(0.7);
  expect(v.x).toBeGreaterThan(0);
  expect(v.y).toBeGreaterThan(0);
  // A drawing that fits above the floor is centred as before.
  const small = fitViewport({ x0: 0, y0: 0, x1: 300, y1: 200 }, 700, 480, 1.15, 0.7);
  expect(small).toEqual(fitViewport({ x0: 0, y0: 0, x1: 300, y1: 200 }, 700, 480));
});
