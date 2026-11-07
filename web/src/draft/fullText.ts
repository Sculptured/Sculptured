import { CARD_H, CARD_W } from '../layout';
import type { TeamDraft } from '../generated/TeamDraft';

const GROUP_PAD = 20;

/**
 * The board in full-text mode, for drawing only: each card takes its measured height (`heights`, at least
 * CARD_H), a card below a taller one in the same column moves down by the extra height (keeping the gap it
 * had), and group frames move and grow with their members. The saved draft is never changed.
 */
export function fullTextLayout(d: TeamDraft, heights: Record<string, number>): TeamDraft {
  const h = (id: string) => Math.max(CARD_H, heights[id] ?? CARD_H);
  const y = agentTops(d.agents, h);
  const frame = (g: (typeof d.groups)[number]) => {
    const members = d.agents.filter((a) => a.group_id === g.id);
    if (!members.length) return g;
    const shift = Math.min(...members.map((a) => y.get(a.id)! - a.y));
    const top = g.y + shift;
    const bottom = Math.max(...members.map((a) => y.get(a.id)! + h(a.id))) + GROUP_PAD;
    return { ...g, y: top, h: Math.max(g.h, bottom - top) };
  };
  // A group that was below another in the same column moves down past it, members and all (keeping the gap).
  const placed = new Map<string, (typeof d.groups)[number]>();
  for (const g of [...d.groups].sort((a, b) => a.y - b.y || a.x - b.x)) {
    let next = frame(g);
    for (const p of d.groups) {
      const q = placed.get(p.id);
      const sameColumn = p.x < g.x + g.w && g.x < p.x + p.w, wasAbove = p.y + p.h <= g.y + 1;
      if (!q || !sameColumn || !wasAbove) continue;
      const delta = q.y + q.h + (g.y - (p.y + p.h)) - next.y;
      if (delta > 0) {
        for (const a of d.agents) if (a.group_id === g.id) y.set(a.id, y.get(a.id)! + delta);
        next = { ...next, y: next.y + delta };
      }
    }
    placed.set(g.id, next);
  }
  const agents = d.agents.map((a) => ({ ...a, y: y.get(a.id)! }));
  return { ...d, agents, groups: d.groups.map((g) => placed.get(g.id) ?? g) };
}

/** Expand cards down their columns while keeping each original gap. */
function agentTops(agents: TeamDraft['agents'], height: (id: string) => number): Map<string, number> {
  const order = [...agents].sort((a, b) => a.y - b.y || a.x - b.x);
  const y = new Map<string, number>();
  for (const a of order) {
    let top = a.y;
    for (const b of order) {
      if (b === a || !y.has(b.id)) continue;
      const sameColumn = Math.abs(b.x - a.x) < CARD_W, wasAbove = b.y + CARD_H <= a.y + 1;
      if (sameColumn && wasAbove) top = Math.max(top, y.get(b.id)! + height(b.id) + (a.y - (b.y + CARD_H)));
    }
    y.set(a.id, top);
  }
  return y;
}
