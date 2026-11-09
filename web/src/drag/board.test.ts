import { describe, expect, it } from 'vitest';
import type { TeamDraft } from '../generated/TeamDraft';
import { draftAgent, teamDraft } from '../test-fixtures';
import { agentBox, drawnGroups, overlaps } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { adding, agentDrag, freeTarget, gap, groupDrag, landing, lerpDraft, moving, selectionDrag, slots, slotTargets, SNAP } from './board';

const box = (id: string, x: number, y: number, w = 248, h = 276) => ({ id, label: id, x, y, w, h });
/** Two drawn groups and one agent outside both. */
const grouped = (): TeamDraft =>
  teamDraft({
    groups: [box('plan', 40, 40), box('build', 400, 40, 248, 516)],
    agents: [
      draftAgent('planner', { group_id: 'plan', x: 64, y: 92 }),
      draftAgent('one', { group_id: 'build', x: 424, y: 92 }),
      draftAgent('two', { group_id: 'build', x: 424, y: 332 }),
      draftAgent('loose', { x: 760, y: 92 }),
    ],
    connections: [],
  });

const agent = (d: TeamDraft, id: string) => d.agents.find((a) => a.id === id)!;
const clear = (d: TeamDraft) => d.agents.every((a, i) => d.agents.every((b, j) => j <= i || !overlaps(agentBox(a), agentBox(b))));
const groupsApart = (d: TeamDraft) => drawnGroups(d).every((a, i) => drawnGroups(d).every((b, j) => j <= i || !overlaps(a, b)));

describe('the states an agent drag can reach', () => {
  it('lists where it is, beside every other card, and each group’s first free place', () => {
    const d = teamDraft();
    const s = slots(d, 'builder').map((x) => x.at);
    expect(s[0]).toEqual({ x: 40, y: 40 });
    // Beside Reviewer (300, 40) at the board's spacing; above it would leave the canvas.
    expect(s).toEqual(expect.arrayContaining([{ x: 564, y: 40 }, { x: 36, y: 40 }, { x: 300, y: 280 }]));
    expect(s.every((p) => p.x >= 0 && p.y >= 0)).toBe(true);
  });

  it('offers only valid states: on the canvas, on no other card, groups never covering each other', () => {
    const d = grouped();
    for (const id of ['planner', 'one', 'loose']) {
      const targets = slotTargets(d, id, moving(id, false));
      expect(targets.length).toBeGreaterThan(3);
      for (const t of targets) {
        expect(clear(t.state)).toBe(true);
        expect(groupsApart(t.state)).toBe(true);
        expect(agent(t.state, id)).toMatchObject(t.at);
      }
      expect(new Set(targets.map((t) => t.key)).size).toBe(targets.length);
    }
  });

  it('a running team never offers a state where the agent changes group', () => {
    const d = grouped();
    const targets = slotTargets(d, 'one', moving('one', true));
    expect(targets.length).toBeGreaterThan(0);
    expect(targets.every((t) => agent(t.state, 'one').group_id === 'build')).toBe(true);
    // The same drop on Planning is refused, so it is not a target at all.
    expect(landing(moving('one', true)(d, { x: 64, y: 100 }))).toBeNull();
  });

  it('while the main group is drawn, no state leaves an agent outside every group', () => {
    const d = teamDraft({
      groups: [{ ...box('team', 40, 40), label: 'Core' }, box('side', 400, 40)],
      agents: [draftAgent('a', { x: 64, y: 92 }), draftAgent('b', { group_id: 'side', x: 424, y: 92 })],
      connections: [],
    });
    const ids = new Set(drawnGroups(d).map((g) => g.id));
    for (const t of slotTargets(d, 'b', moving('b', false))) expect(ids.has(groupOf(agent(t.state, 'b')))).toBe(true);
  });

  it('every frame of a drag, anywhere on the board, is a valid state', () => {
    const d = grouped();
    for (const id of ['planner', 'two', 'loose']) {
      const drag = agentDrag(d, id, moving(id, false));
      for (let x = -100; x < 1100; x += 37)
        for (let y = -100; y < 800; y += 41) {
          const f = drag({ x, y })!;
          expect(f).not.toBeNull();
          expect(clear(f.state)).toBe(true);
          expect(groupsApart(f.state)).toBe(true);
        }
    }
  });
});

describe('dragging an agent', () => {
  it('near a group’s free place it snaps in: the group grows, the one it left shrinks', () => {
    const d = grouped();
    const drag = agentDrag(d, 'loose', moving('loose', false));
    // Build's first free place is below Two, at (424, 572).
    const f = drag({ x: 430, y: 560 })!;
    expect(f.at).toEqual({ x: 424, y: 572 });
    expect(agent(f.state, 'loose').group_id).toBe('build');
    const build = f.state.groups.find((g) => g.id === 'build')!;
    expect(build.y + build.h).toBeGreaterThanOrEqual(572 + 200);
    // Floating: drawn near the pointer, pulled toward the slot.
    expect(f.drawAt.x).toBeGreaterThanOrEqual(424);
    expect(f.drawAt.x).toBeLessThanOrEqual(430);

    const out = agentDrag(d, 'two', moving('two', false))({ x: 900, y: 500 })!;
    expect(agent(out.state, 'two').group_id).toBeNull();
    expect(out.state.groups.find((g) => g.id === 'build')!.h).toBeLessThan(516);
  });

  it('far from every slot it goes where it is dropped; over another card, to the nearest clear place', () => {
    const d = teamDraft();
    const place = moving('builder', false);
    expect(freeTarget(d, 'builder', place, { x: 120, y: 400 })?.at).toEqual({ x: 120, y: 400 });
    const nudged = freeTarget(d, 'builder', place, { x: 120, y: 200 })!;
    expect(nudged.at).toEqual({ x: 84, y: 200 });
    expect(overlaps(agentBox(nudged.at), agentBox(agent(d, 'reviewer')))).toBe(false);
    expect(agentDrag(d, 'builder', place)({ x: 120 + SNAP * 4, y: 420 })?.at).toEqual({ x: 120 + SNAP * 4, y: 420 });
  });

  it('a new agent from the palette exists only in the previewed states', () => {
    const d = teamDraft();
    const f = agentDrag(d, null, adding('codex'))({ x: 100, y: 400 })!;
    expect(d.agents).toHaveLength(2);
    expect(f.state.agents).toHaveLength(3);
    expect(f.state.agents[2]).toMatchObject({ kind: 'codex', x: 100, y: 400 });
  });
});

describe('dragging groups and selections', () => {
  it('a group goes toward the pointer but stops short of covering another group', () => {
    const d = grouped();
    const drag = groupDrag(d, 'plan');
    expect(drag({ x: 40, y: 400 })!.at).toEqual({ x: 40, y: 400 });
    const f = drag({ x: 380, y: 40 })!;
    expect(f.at.x).toBeLessThan(400 - 248);
    expect(groupsApart(f.state)).toBe(true);
    expect(agent(f.state, 'planner').x - f.at.x).toBe(24);
  });

  it('several agents move together, and a spot that would land one on a card left behind is skipped', () => {
    const d = teamDraft({ agents: [draftAgent('a', { x: 40, y: 40 }), draftAgent('b', { x: 40, y: 300 }), draftAgent('c', { x: 600, y: 40 })], connections: [] });
    const drag = selectionDrag(d, ['a', 'b'], 'a', false);
    const ok = drag({ x: 300, y: 40 })!;
    expect([agent(ok.state, 'a').x, agent(ok.state, 'b').x]).toEqual([300, 300]);
    // At x 500, a would cover c: the last valid preview stays.
    expect(drag({ x: 500, y: 40 })).toBe(ok);
  });
});

describe('animating between states', () => {
  it('blends card and group geometry, draws the pinned card where it is going, and measures what is left', () => {
    const a = grouped();
    const b = { ...a, agents: a.agents.map((x) => (x.id === 'loose' ? { ...x, x: 860, y: 192 } : x.id === 'one' ? { ...x, x: 524 } : x)) };
    const half = lerpDraft(a, b, 0.5, 'loose');
    expect(agent(half, 'one').x).toBe(474);
    expect(agent(half, 'loose')).toMatchObject({ x: 860, y: 192 });
    expect(gap(a, b)).toBe(100);
    expect(gap(b, b)).toBe(0);
    expect(lerpDraft(b, b, 0.3)).toBe(b);
  });
});
