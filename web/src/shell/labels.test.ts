import { describe, expect, it } from 'vitest';
import type { Box } from '../layout';
import { edgeGeometry } from './LiveGraph';
import { bezierPoints, labelSize, placeLabels, type LabelSpot } from './labels';

const card = (x: number, y: number): Box => ({ x, y, w: 200, h: 200 });
const rect = (s: LabelSpot) => ({ x0: s.x - s.w / 2, x1: s.x + s.w / 2, y0: s.y - s.h / 2, y1: s.y + s.h / 2 });
const inside = (p: { x: number; y: number }, s: LabelSpot, pad = 0) => {
  const r = rect(s);
  return p.x > r.x0 - pad && p.x < r.x1 + pad && p.y > r.y0 - pad && p.y < r.y1 + pad;
};
const overlapsBox = (s: LabelSpot, b: Box) => {
  const r = rect(s);
  return r.x0 < b.x + b.w && b.x < r.x1 && r.y0 < b.y + b.h && b.y < r.y1;
};

function check(boxes: Box[], pairs: [number, number][]) {
  const items = pairs.map(([a, b], i) => ({ key: `e${i}`, geom: edgeGeometry(boxes[a], boxes[b]), text: 'hands work to' }));
  const spots = placeLabels(items, boxes);
  for (const it of items) {
    const s = spots.get(it.key)!;
    expect(s, it.key).toBeDefined();
    // Never on a curve: neither its own nor any other connection's.
    for (const other of items) for (const p of bezierPoints(other.geom.pts, 96)) expect(inside(p, s, 2), `${it.key} on ${other.key}`).toBe(false);
    // Never on a card.
    for (const b of boxes) expect(overlapsBox(s, b), `${it.key} on a card`).toBe(false);
  }
  // Never on each other.
  const all = [...spots.values()];
  for (let i = 0; i < all.length; i++)
    for (let j = i + 1; j < all.length; j++) {
      const a = rect(all[i]);
      const b = rect(all[j]);
      expect(a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1).toBe(false);
    }
  return spots;
}

describe('connection labels sit beside their curve, never on it', () => {
  it('two cards side by side with a wide gap: the label sits just above the line', () => {
    const spots = check([card(0, 0), card(400, 0)], [[0, 1]]);
    const s = spots.get('e0')!;
    expect(s.y).toBeLessThan(100);
    expect(s.x).toBeGreaterThan(200);
    expect(s.x).toBeLessThan(400);
  });

  it('two cards 60px apart (templates): the wrapped label sits in the gap, above the line and clear of the ports', () => {
    const spots = check([card(40, 40), card(300, 40)], [[0, 1]]);
    const s = spots.get('e0')!;
    expect(s.wrap).toBe(true);
    expect(rect(s).y1).toBeLessThanOrEqual(140 - 9);
  });

  it('one agent handing work to two (fan-out through a narrow gap): each label clears both curves', () => {
    check([card(0, 150), card(276, 0), card(276, 300)], [
      [0, 1],
      [0, 2],
    ]);
  });

  it('two agents handing work to one (fan-in)', () => {
    check([card(0, 0), card(0, 300), card(276, 150)], [
      [0, 2],
      [1, 2],
    ]);
  });

  it('a chain across a wide layout and a return connection below', () => {
    check([card(0, 0), card(320, 0), card(640, 0)], [
      [0, 1],
      [1, 2],
      [2, 0],
    ]);
  });

  it('a card dropped right against another (no gap at all): the label still finds a clear spot', () => {
    // Seen in the browser: Practice 1 dropped touching Reviewer's bottom edge, offset to the left.
    check([card(0, 0), card(260, 0), card(130, 200)], [
      [0, 1],
      [1, 2],
    ]);
  });

  it('vertical neighbours', () => {
    check([card(0, 0), card(0, 320)], [[0, 1]]);
  });

  it('estimates a one-line label for a wide gap and a wrapped one for a narrow gap', () => {
    expect(labelSize('hands work to', Infinity)).toMatchObject({ wrap: false, h: 22 });
    const narrow = labelSize('hands work to', 60);
    expect(narrow.wrap).toBe(true);
    expect(narrow.w).toBeLessThanOrEqual(60);
    expect(narrow.h).toBeGreaterThan(22);
  });

  it('a label whose words cannot fit a narrow gap stays on one line', () => {
    const s = labelSize('1 message observed', 60);
    expect(s.wrap).toBe(false);
    expect(s.h).toBe(22);
  });

  it('"1 message observed" between two cards 60px apart sits clear of both cards and every line', () => {
    const boxes = [card(40, 40), card(300, 40)];
    const items = [{ key: 'e0', geom: edgeGeometry(boxes[0], boxes[1]), text: '1 message observed' }];
    const s = placeLabels(items, boxes).get('e0')!;
    for (const b of boxes) expect(overlapsBox(s, b)).toBe(false);
    for (const p of bezierPoints(items[0].geom.pts, 96)) expect(inside(p, s, 2)).toBe(false);
    // Above or below the pair, centred on the gap, not off in a corner.
    expect(rect(s).y1 <= 40 || rect(s).y0 >= 240).toBe(true);
    expect(s.x).toBeGreaterThan(240);
    expect(s.x).toBeLessThan(300);
  });

  it('observed-message labels in a fan-out with narrow gaps never sit on a card', () => {
    const boxes = [card(0, 150), card(260, 0), card(260, 300)];
    const items = [[0, 1], [0, 2]].map(([a, b], i) => ({ key: `e${i}`, geom: edgeGeometry(boxes[a], boxes[b]), text: `${i + 2} messages observed` }));
    const spots = placeLabels(items, boxes);
    for (const it of items) for (const b of boxes) expect(overlapsBox(spots.get(it.key)!, b), it.key).toBe(false);
  });
});
