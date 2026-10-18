import type { Box } from '../layout';

/**
 * Where each connection label goes. A label never sits on a curve (its own or another's), on
 * a card, on a port or on another label: it sits beside its curve, as close to the middle as
 * it can. In a gap too narrow for the label to sit beside the curve, it sits above or below
 * the part of the curve that crosses the gap. If cards are dropped so close that no spot is clear,
 * it takes the least crowded one (a card counts worst). A label whose words cannot fit the gap
 * stays on one line and sits just above or below the cards it would otherwise cover. Pure
 * geometry, tested without a browser.
 */

export type Point = { x: number; y: number };
export type Bezier = [Point, Point, Point, Point];

/** A label's centre and its estimated size; `wrap` labels break onto more than one line. */
export type LabelSpot = { x: number; y: number; w: number; h: number; wrap: boolean; maxWidth?: number };

/** Wide enough for "hands work to" on one line; a narrower gap wraps it. */
export const LABEL_ONE_LINE = 116;
/** Free space kept between a label and a curve, and between a label and a port's centre. */
const CLEAR = 5;
const PORT = 10;
const LINE_H = 22;
const WRAP_LINE_H = 14;
/** Per character of the 11.5px label face, generously, so the estimate is never too small. */
const CHAR_W = 6.9;

export function bezierAt([p0, p1, p2, p3]: Bezier, t: number): Point {
  const u = 1 - t;
  const a = u * u * u;
  const b = 3 * u * u * t;
  const c = 3 * u * t * t;
  const d = t * t * t;
  return { x: a * p0.x + b * p1.x + c * p2.x + d * p3.x, y: a * p0.y + b * p1.y + c * p2.y + d * p3.y };
}

function tangentAt([p0, p1, p2, p3]: Bezier, t: number): Point {
  const u = 1 - t;
  const x = 3 * u * u * (p1.x - p0.x) + 6 * u * t * (p2.x - p1.x) + 3 * t * t * (p3.x - p2.x);
  const y = 3 * u * u * (p1.y - p0.y) + 6 * u * t * (p2.y - p1.y) + 3 * t * t * (p3.y - p2.y);
  const n = Math.hypot(x, y) || 1;
  return { x: x / n, y: y / n };
}

export function bezierPoints(b: Bezier, n = 64): Point[] {
  return Array.from({ length: n + 1 }, (_, i) => bezierAt(b, i / n));
}

/** The label's size: one line when the gap allows it, else wrapped to fit the gap. */
export function labelSize(text: string, room: number): { w: number; h: number; wrap: boolean; maxWidth?: number } {
  const one = Math.ceil(text.length * CHAR_W + 20);
  if (room >= LABEL_ONE_LINE) return { w: one, h: LINE_H, wrap: false };
  const maxWidth = Math.max(60, room - 2);
  // A word wider than the gap would spill onto the cards: keep the label on one line and let
  // placeLabels set it above or below the cards instead of squeezing it between them.
  const longest = Math.max(...text.split(/\s+/).map((w) => w.length));
  if (longest * CHAR_W + 14 > maxWidth) return { w: one, h: LINE_H, wrap: false };
  // Wrapped labels have 6px side padding and a 1px border.
  const lines = Math.max(2, Math.ceil((text.length * CHAR_W) / (maxWidth - 14)));
  return { w: maxWidth, h: lines * WRAP_LINE_H + 6, wrap: true, maxWidth };
}

type R = { x0: number; y0: number; x1: number; y1: number };
const rectOf = (c: Point, w: number, h: number): R => ({ x0: c.x - w / 2, y0: c.y - h / 2, x1: c.x + w / 2, y1: c.y + h / 2 });
const hits = (a: R, b: R) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
const near = (r: R, p: Point, pad: number) => p.x > r.x0 - pad && p.x < r.x1 + pad && p.y > r.y0 - pad && p.y < r.y1 + pad;

export type LabelItem = { key: string; geom: { pts: Bezier; mid: Point; room: number }; text: string };

export function placeLabels(items: LabelItem[], boxes: Box[]): Map<string, LabelSpot> {
  const out = new Map<string, LabelSpot>();
  const curves = items.map((it) => bezierPoints(it.geom.pts, 96));
  const ports = items.flatMap((it) => [it.geom.pts[0], it.geom.pts[3]]);
  const cards: R[] = boxes.map((b) => ({ x0: b.x, y0: b.y, x1: b.x + b.w, y1: b.y + b.h }));
  const placed: R[] = [];

  // How badly a spot collides: a card is worst, then another label, then a port, then a line.
  const cost = (r: R) =>
    1000 * cards.filter((c) => hits(r, c)).length +
    100 * placed.filter((p) => hits({ x0: r.x0 - 4, y0: r.y0 - 4, x1: r.x1 + 4, y1: r.y1 + 4 }, p)).length +
    10 * ports.filter((p) => near(r, p, PORT)).length +
    curves.reduce((n, pts) => n + pts.filter((p) => near(r, p, CLEAR)).length, 0);

  for (const it of items) {
    const { w, h, wrap, maxWidth } = labelSize(it.text, it.geom.room);
    const b = it.geom.pts;
    const candidates: Point[] = [];
    // Beside the curve, near its middle first: offset along the normal, upper side first.
    for (const t of [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74]) {
      const p = bezierAt(b, t);
      const tan = tangentAt(b, t);
      let n = { x: -tan.y, y: tan.x };
      if (n.y > 0 || (n.y === 0 && n.x > 0)) n = { x: -n.x, y: -n.y };
      const d = (Math.abs(n.x) * w + Math.abs(n.y) * h) / 2 + CLEAR + 2;
      candidates.push({ x: p.x + n.x * d, y: p.y + n.y * d }, { x: p.x - n.x * d, y: p.y - n.y * d });
    }
    // Above or below the stretch of curve under the label (a narrow gap between two cards).
    const cx = it.geom.mid.x;
    const under = bezierPoints(b, 96).filter((p) => p.x >= cx - w / 2 - CLEAR && p.x <= cx + w / 2 + CLEAR);
    if (under.length > 0) {
      const top = Math.min(...under.map((p) => p.y));
      const bottom = Math.max(...under.map((p) => p.y));
      for (const extra of [0, 8, 20]) {
        candidates.push({ x: cx, y: top - h / 2 - PORT - extra }, { x: cx, y: bottom + h / 2 + PORT + extra });
      }
    }
    // Farther out along the normal at the middle, for crowded places.
    {
      const p = bezierAt(b, 0.5);
      const tan = tangentAt(b, 0.5);
      const n = { x: -tan.y, y: tan.x };
      const d = (Math.abs(n.x) * w + Math.abs(n.y) * h) / 2 + CLEAR + 2;
      for (const extra of [16, 36, 64]) {
        candidates.push({ x: p.x + n.x * (d + extra), y: p.y + n.y * (d + extra) }, { x: p.x - n.x * (d + extra), y: p.y - n.y * (d + extra) });
      }
    }
    // Clear of the cards: just above the highest, or just below the lowest, card under the label.
    {
      const span = (y: number): R => rectOf({ x: cx, y }, w, h);
      const under = cards.filter((c) => c.x0 < cx + w / 2 && cx - w / 2 < c.x1);
      if (under.length > 0) {
        const top = Math.min(...under.map((c) => c.y0));
        const bottom = Math.max(...under.map((c) => c.y1));
        for (const extra of [0, 12]) {
          const above = top - h / 2 - CLEAR - extra;
          const below = bottom + h / 2 + CLEAR + extra;
          if (cards.every((c) => !hits(span(above), c))) candidates.push({ x: cx, y: above });
          if (cards.every((c) => !hits(span(below), c))) candidates.push({ x: cx, y: below });
        }
      }
    }
    // Last resort (cards dropped against each other): a ring of spots around the middle.
    const m = it.geom.mid;
    for (const ring of [1, 2, 3])
      for (let k = 0; k < 16; k++) {
        const a = (k / 16) * 2 * Math.PI;
        candidates.push({ x: m.x + Math.cos(a) * (w / 2 + 14) * ring, y: m.y + Math.sin(a) * (h / 2 + 14) * ring });
      }
    // The first clear spot, else the least bad one (never the middle of the line by default).
    let spot = candidates.find((c) => cost(rectOf(c, w, h)) === 0);
    if (!spot) {
      let best = Infinity;
      for (const c of [...candidates, m]) {
        const v = cost(rectOf(c, w, h));
        if (v < best) [best, spot] = [v, c];
      }
    }
    spot ??= m;
    placed.push(rectOf(spot, w, h));
    out.set(it.key, { x: spot.x, y: spot.y, w, h, wrap, maxWidth });
  }
  return out;
}
