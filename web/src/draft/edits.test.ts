import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { draftAgent, teamDraft } from '../test-fixtures';
import {
  addAgent,
  addGroup,
  connect,
  fitGroups,
  groupsFit,
  moveAgent,
  moveGroup,
  moveToGroup,
  removeElement,
  renameAgent,
  selectionExists,
  setAdvanced,
  setConnectionKind,
  setFolder,
  setJob,
  takeOut,
  ungroup,
} from './edits';
import { agentAt, alignCard, canvasSize, draftLayout, drawnGroups, nextFreeSlot, snap } from './geometry';
import { DEFAULT_GROUP_ID, nameMap, nextAgentName, seatIdFor, slug } from './ids';

describe('ids', () => {
  it('slugs names and picks the next free default name', () => {
    expect(slug('Claude 1', 'agent')).toBe('claude-1');
    expect(slug('  ', 'agent')).toBe('agent');
    expect(nextAgentName(teamDraft(), 'claude')).toBe('Claude 1');
    const base = teamDraft();
    const d = { ...base, agents: [...base.agents, { ...base.agents[0], id: 'x', name: 'claude 1' }] };
    expect(nextAgentName(d, 'claude')).toBe('Claude 2');
    expect(nextAgentName(d, 'practice')).toBe('Practice 1');
  });

  it('derives seat ids and names from immutable ids', () => {
    const d = teamDraft();
    expect(DEFAULT_GROUP_ID).toBe('team');
    expect(seatIdFor(d, 'builder')).toBe('team.builder@t');
    expect(seatIdFor(d, 'nobody')).toBeNull();
    expect(nameMap(d)).toEqual({ 'team.builder@t': 'Builder', 'team.reviewer@t': 'Reviewer' });
  });
});

describe('edits', () => {
  it('adds an agent with a default name, an id from it, where it was dropped', () => {
    const { draft, id } = addAgent(teamDraft(), 'claude', { x: 301, y: 283 });
    const a = draft.agents.find((x) => x.id === id)!;
    expect(a.name).toBe('Claude 1');
    expect(id).toMatch(/^claude-1-[0-9a-z]{4}$/);
    expect([a.x, a.y]).toEqual([301, 283]);
    expect(a.group_id).toBeNull();
    expect(a.advanced).toEqual({ model: null, permission: 'unrestricted', resume: 'resume-if-possible' });
  });

  it('moves to whole units and never below zero', () => {
    expect(moveAgent(teamDraft(), 'builder', { x: -50, y: 13.4 }).draft.agents[0]).toMatchObject({ x: 0, y: 13 });
  });

  it('deleting an agent or a whole group removes its agents and connections', () => {
    expect(removeElement(teamDraft(), { kind: 'agent', id: 'builder' }).draft.connections).toEqual([]);
    expect(removeElement(teamDraft(), { kind: 'connection', id: 'c1' }).draft.connections).toEqual([]);
    const added = addGroup(teamDraft());
    const grouped = moveToGroup(added.draft, 'builder', added.id);
    const r = removeElement(grouped, { kind: 'group', id: added.id });
    expect(r.reason).toBeUndefined();
    expect(r.draft.agents.map((a) => a.id)).toEqual(['reviewer']);
    expect(r.draft.connections).toHaveLength(0);
    expect(grouped.agents).toHaveLength(2);
  });

  it('refuses self and duplicate connections with a reason', () => {
    expect(connect(teamDraft(), 'builder', 'builder').reason).toBe('An agent cannot hand work to itself.');
    expect(connect(teamDraft(), 'builder', 'reviewer').reason).toBe('Builder already hands work to Reviewer.');
    const r = connect(teamDraft(), 'reviewer', 'builder');
    expect(r.draft.connections.at(-1)).toMatchObject({ id: r.id, from: 'reviewer', to: 'builder', kind: 'hands-work-to' });
    expect(setConnectionKind(r.draft, r.id!, 'watches').draft.connections.at(-1)!.kind).toBe('watches');
  });

  it('the first + Group adds one empty group; the other agents stay outside it and keep seat ids', () => {
    const before = teamDraft();
    const { draft, id } = addGroup(before);
    expect(draft.groups.map((g) => g.label)).toEqual(['Group 1']);
    expect(draft.groups.some((g) => g.id === DEFAULT_GROUP_ID)).toBe(false);
    expect(draft.agents.every((a) => a.group_id === null)).toBe(true);
    expect(seatIdFor(draft, 'builder')).toBe(seatIdFor(before, 'builder'));
    expect(drawnGroups(draft)).toHaveLength(1);
    expect(draft.groups.find((g) => g.id === id)!.x).toBeGreaterThanOrEqual(Math.max(...before.agents.map((a) => a.x + 200)));
  });

  it('dragging a card out of its group onto empty board takes it out, where it was dropped', () => {
    const { draft, id } = addGroup(teamDraft());
    const grouped = moveToGroup(draft, 'builder', id);
    const g = grouped.groups.find((x) => x.id === id)!;
    const out = moveAgent(grouped, 'builder', { x: 40, y: 700 }).draft;
    expect(out.agents[0]).toMatchObject({ x: 40, y: 700, group_id: null });
    expect(out.groups.find((x) => x.id === id)!.h).toBeLessThanOrEqual(g.h);
  });

  it('a card dropped half over a group box, centre outside, moves out from under the box', () => {
    const { draft, id } = addGroup(teamDraft());
    const g = draft.groups.find((x) => x.id === id)!;
    const a = moveAgent(draft, 'reviewer', { x: g.x - 120, y: g.y }).draft.agents[1];
    expect(a.group_id).toBeNull();
    expect(a.x + 200).toBeLessThanOrEqual(g.x);
  });

  it('Ungroup removes the box and keeps its agents; Take out moves one agent out', () => {
    const { draft, id } = addGroup(teamDraft());
    const both = moveToGroup(moveToGroup(draft, 'builder', id), 'reviewer', id);
    const loose = ungroup(both, id);
    expect(loose.groups).toEqual([]);
    expect(loose.agents.map((a) => a.group_id)).toEqual([null, null]);
    const one = takeOut(both, 'builder');
    const g = one.groups.find((x) => x.id === id)!;
    const b = one.agents[0];
    expect(b.group_id).toBeNull();
    expect(b.x >= g.x + g.w || b.y >= g.y + g.h || b.x + 200 <= g.x || b.y + 200 <= g.y).toBe(true);
    expect(one.agents[1].group_id).toBe(id);
  });

  it('moves an agent into a group inside its box, and a group with its agents', () => {
    const { draft, id } = addGroup(teamDraft());
    const moved = moveToGroup(draft, 'builder', id);
    const g = moved.groups.find((x) => x.id === id)!;
    const a = moved.agents[0];
    expect(a.group_id).toBe(id);
    expect(a.x).toBeGreaterThanOrEqual(g.x);
    expect(a.y).toBeGreaterThanOrEqual(g.y);
    expect(a.x + 200).toBeLessThanOrEqual(g.x + g.w);
    const shifted = moveGroup(moved, id, 32, 16);
    expect(shifted.agents[0]).toMatchObject({ x: a.x + 32, y: a.y + 16 });
    expect(shifted.groups.find((x) => x.id === id)).toMatchObject({ x: g.x + 32, y: g.y + 16 });
  });

  it('dragging an agent onto a drawn group joins it', () => {
    const { draft, id } = addGroup(teamDraft());
    const g = draft.groups.find((x) => x.id === id)!;
    expect(moveAgent(draft, 'reviewer', { x: g.x + 24, y: g.y + 52 }).draft.agents[1].group_id).toBe(id);
  });

  it('a move that must keep its group (team running) refuses a drop on another group and snaps back', () => {
    const { draft, id } = addGroup(teamDraft());
    const g = draft.groups.find((x) => x.id === id)!;
    const r = moveAgent(draft, 'reviewer', { x: g.x + 24, y: g.y + 52 }, true);
    expect(r.draft).toBe(draft);
    expect(r.reason).toBe("An agent can't change group while the team is running.");
  });

  it('a move that must keep its group (team running) still moves the card, and a grouped card stays in its group', () => {
    const { draft, id } = addGroup(teamDraft());
    const r = moveAgent(draft, 'reviewer', { x: 304, y: 64 }, true);
    expect(r.reason).toBeUndefined();
    expect(r.draft.agents[1]).toMatchObject({ x: 304, y: 64, group_id: null });
    const grouped = moveToGroup(draft, 'builder', id);
    expect(moveAgent(grouped, 'builder', { x: 40, y: 700 }, true).draft.agents[0].group_id).toBe(id);
  });

  it('sets fields and forgets selections of removed elements', () => {
    let d = renameAgent(teamDraft(), 'builder', 'Lead');
    d = setJob(d, 'builder', 'Do it');
    d = setFolder(d, 'builder', '/home/u/repo');
    d = setAdvanced(d, 'builder', { permission: 'restricted' });
    expect(d.agents[0]).toMatchObject({ name: 'Lead', job: 'Do it', folder: '/home/u/repo' });
    expect(d.agents[0].advanced).toEqual({ model: null, permission: 'restricted', resume: 'resume-if-possible' });
    expect(selectionExists(d, { kind: 'agent', id: 'builder' })).toEqual({ kind: 'agent', id: 'builder' });
    expect(selectionExists(d, { kind: 'agent', id: 'gone' })).toBeNull();
  });
});

describe('geometry', () => {
  it('finds the next free slot and the agent under a point', () => {
    expect(nextFreeSlot(teamDraft())).toEqual({ x: 568, y: 40 });
    expect(agentAt(teamDraft(), { x: 350, y: 100 })).toBe('reviewer');
    expect(agentAt(teamDraft(), { x: 270, y: 100 })).toBeNull();
    expect(snap(12.4)).toBe(12);
  });

  it('a dragged card lines up with a nearby card within a few units, and goes exactly where dropped otherwise', () => {
    const d = teamDraft();
    const [b, r] = d.agents;
    expect(alignCard(d, 'reviewer', { x: b.x + 7, y: b.y + 300 })).toEqual({ x: b.x, y: b.y + 300 });
    expect(alignCard(d, 'reviewer', { x: b.x + 30, y: b.y + 330 })).toEqual({ x: b.x + 30, y: b.y + 330 });
    expect(alignCard(d, 'reviewer', { x: r.x + 40, y: r.y + 70 })).toEqual({ x: r.x + 40, y: r.y + 70 });
    expect(alignCard(d, 'reviewer', { x: r.x + 3, y: r.y - 5 })).toEqual({ x: b.x + 264, y: b.y });
  });

  it('lays Run mode out from the same positions', () => {
    const l = draftLayout(teamDraft());
    expect(l.seats['team.builder@t']).toEqual({ x: 40, y: 40, w: 200, h: 200 });
    expect(l.seats['team.reviewer@t'].x).toBe(300);
    expect(l.pods).toEqual({});
    expect(l.width).toBe(540);
    expect(canvasSize(teamDraft()).width).toBeGreaterThanOrEqual(960);
  });
});

describe('groupsFit', () => {
  const shipped = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../sculptured-factory/team/ridgeline-incident.json');
  const have = existsSync(shipped);
  const ridgeline = have ? JSON.parse(readFileSync(shipped, 'utf8')) : null;
  const oldGroups = [
    { id: 'incident', label: 'Incident', x: 72, y: 40, w: 1328, h: 260 },
    { id: 'floor', label: 'Plant floor', x: 72, y: 312, w: 1008, h: 260 },
    { id: 'supply', label: 'Supply and insurance', x: 72, y: 584, w: 624, h: 260 },
    { id: 'customers', label: 'Customers', x: 712, y: 584, w: 304, h: 260 },
    { id: 'money', label: 'Money', x: 1032, y: 584, w: 368, h: 260 },
  ];
  const at = (id: string, group_id: string, x: number, y: number) => draftAgent(id, { group_id, x, y });
  const imported = teamDraft({
    groups: oldGroups,
    agents: [
      at('lead', 'incident', 96, 112),
      at('maintenance', 'floor', 96, 384),
      at('ehs', 'floor', 416, 384),
      at('planning', 'floor', 736, 384),
      at('procurement', 'supply', 96, 656),
      at('insurance', 'supply', 416, 656),
      at('customers', 'customers', 736, 656),
      at('finance', 'money', 1056, 656),
    ],
  });
  it('is false for a team whose cards stick out of their groups', () => {
    expect(groupsFit(imported)).toBe(false);
  });
  it('is true once the groups are fitted', () => {
    expect(groupsFit(fitGroups(imported))).toBe(true);
  });
  it('fits idempotently', () => {
    expect(fitGroups(fitGroups(imported))).toEqual(fitGroups(imported));
  });
  it.skipIf(!have)('holds for the shipped Ridgeline file', () => {
    expect(groupsFit(ridgeline.team)).toBe(true);
  });
});
