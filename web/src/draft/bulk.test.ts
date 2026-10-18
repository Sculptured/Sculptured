import { describe, expect, it } from 'vitest';
import { draftAgent, teamDraft } from '../test-fixtures';
import { addGroup } from './edits';
import { groupAgents, moveAgents, moveAgentsToGroup, removeAgents, setFolderFor, setKindFor, setLimitsFor, setModelFor } from './bulk';
import { RUNNING_GROUP_REASON } from './edits';
import type { TeamDraft } from '../generated/TeamDraft';

const three = () =>
  teamDraft({
    agents: [draftAgent('a', { x: 40, y: 40, kind: 'claude' }), draftAgent('b', { x: 340, y: 40, kind: 'claude' }), draftAgent('c', { x: 640, y: 40, kind: 'codex' })],
    connections: [
      { id: 'c1', from: 'a', to: 'b', kind: 'hands-work-to' },
      { id: 'c2', from: 'b', to: 'c', kind: 'hands-work-to' },
    ],
  });
const at = (d: TeamDraft, id: string) => d.agents.find((a) => a.id === id)!;
const asDraft = (r: TeamDraft | { draft: TeamDraft; reason?: string }) => ('agents' in r ? r : r.draft);

describe('bulk edits', () => {
  it('moves the agents together and leaves the others where they are', () => {
    const d = asDraft(moveAgents(three(), ['a', 'b'], 32, 16));
    expect([at(d, 'a').x, at(d, 'a').y, at(d, 'b').x, at(d, 'b').y]).toEqual([72, 56, 372, 56]);
    expect([at(d, 'c').x, at(d, 'c').y]).toEqual([640, 40]);
  });

  it('refuses the whole move, with the reason, when one agent of a running team would change group', () => {
    const g = addGroup(three(), { x: 600, y: 0 });
    const d = moveAgentsToGroup(g.draft, ['c'], g.id);
    const c = at(d, 'c');
    // b lands on the group c is in: a running team cannot change groups.
    const r = moveAgents(d, ['a', 'b'], c.x - at(d, 'b').x, c.y - at(d, 'b').y, true);
    expect('reason' in r && r.reason).toBe(RUNNING_GROUP_REASON);
    expect('draft' in r && r.draft).toBe(d);
  });

  it('deletes the agents and every arrow to or from them', () => {
    const d = removeAgents(three(), ['a', 'c']);
    expect(d.agents.map((a) => a.id)).toEqual(['b']);
    expect(d.connections).toEqual([]);
  });

  it('switches kind and clears each own model; sets a model for each', () => {
    const withModel = setModelFor(three(), ['a', 'b'], 'opus');
    expect([at(withModel, 'a').advanced.model, at(withModel, 'b').advanced.model, at(withModel, 'c').advanced.model]).toEqual(['opus', 'opus', null]);
    const codex = setKindFor(withModel, ['a', 'b', 'missing'], 'codex');
    expect(codex.agents.map((a) => [a.kind, a.advanced.model])).toEqual([['codex', null], ['codex', null], ['codex', null]]);
  });

  it('sets each folder', () => {
    const d = setFolderFor(three(), ['b', 'c'], '/home/me/work');
    expect(d.agents.map((a) => a.folder)).toEqual([null, '/home/me/work', '/home/me/work']);
  });

  it('sets hard limits, leaves untouched what was not given, and removes them with null', () => {
    const d = setLimitsFor(three(), ['a', 'b'], { tokens: 50000, turns: 20 });
    expect(at(d, 'a').advanced.token_limit).toEqual({ tokens: 50000, counts: 'all', include_subagents: true });
    expect(at(d, 'a').advanced.turn_limit).toEqual({ hard: 20 });
    expect(at(d, 'a').advanced.minute_limit).toBeUndefined();
    expect(at(d, 'c').advanced.token_limit).toBeUndefined();
    const minutes = setLimitsFor(d, ['a'], { minutes: 30 });
    expect(at(minutes, 'a').advanced.turn_limit).toEqual({ hard: 20 });
    const cleared = setLimitsFor(minutes, ['a', 'b'], { tokens: null, turns: null, minutes: null });
    expect(JSON.parse(JSON.stringify(at(cleared, 'a').advanced))).toEqual({ model: null, permission: 'unrestricted', resume: 'resume-if-possible' });
  });

  it('puts the agents in a new group of their own, and takes them out again', () => {
    const r = groupAgents(three(), ['a', 'b']);
    expect(r.draft.agents.filter((a) => a.group_id === r.id).map((a) => a.id)).toEqual(['a', 'b']);
    expect(at(r.draft, 'c').group_id).toBeNull();
    const out = moveAgentsToGroup(r.draft, ['a'], null);
    expect(at(out, 'a').group_id).toBeNull();
    expect(at(out, 'b').group_id).toBe(r.id);
  });
});
