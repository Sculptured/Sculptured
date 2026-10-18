import { describe, expect, it } from 'vitest';
import { activityOf, agentState, chipText, shortAge, sortAgents, tierOf, tokensText, vendor } from './herdr';
import { ev, seatView } from './test-fixtures';

describe('agent states', () => {
  it('says an idle agent whose message waits for a slot is waiting, not stuck', () => {
    const held = seatView('a', { lifecycle: 'running', activity: 'idle', slot_wait: true, handoff_stuck: { reason: 'the handoff from Lead was not delivered', can_resend: true } });
    expect(agentState(held).word).toBe('waiting for a slot');
    expect(agentState(seatView('a', { lifecycle: 'running', activity: 'working', slot_wait: true })).key).toBe('working');
  });
  it('maps lifecycle and activity to herdr states, each with a glyph and a word', () => {
    const s = (over: Parameters<typeof seatView>[1]) => agentState(seatView('a', over));
    expect(s({ lifecycle: 'attention' })).toMatchObject({ key: 'needs-you', glyph: '◉', word: 'needs you', tone: 'red' });
    expect(s({ activity: 'waiting-input' })).toMatchObject({ key: 'needs-you' });
    expect(s({ activity: 'working' })).toMatchObject({ key: 'working', glyph: '⣟', word: 'working' });
    expect(s({ activity: 'idle' })).toMatchObject({ key: 'idle', glyph: '○', word: 'idle' });
    expect(agentState(seatView('a'), true)).toMatchObject({ key: 'done', glyph: '✓', word: 'done (reported)', tone: 'reported' });
    expect(s({ activity: 'unknown' })).toMatchObject({ key: 'unknown', glyph: '∅', word: 'unknown', tone: 'violet' });
    expect(s({ lifecycle: 'stale', activity: 'working' })).toMatchObject({ key: 'unknown' });
    expect(agentState(undefined)).toMatchObject({ key: 'unknown' });
  });

  it('shows lifecycle states without a process as muted lifecycle marks; failed is red', () => {
    expect(agentState(seatView('a', { lifecycle: 'launching' }))).toMatchObject({ glyph: '◌', word: 'launching', tone: 'muted' });
    expect(agentState(seatView('a', { lifecycle: 'stopping' }))).toMatchObject({ glyph: '◑', word: 'stopping' });
    expect(agentState(seatView('a', { lifecycle: 'stopped' }))).toMatchObject({ glyph: '■', word: 'stopped' });
    expect(agentState(seatView('a', { lifecycle: 'planned' }))).toMatchObject({ glyph: '○', word: 'planned', tone: 'muted' });
    expect(agentState(seatView('a', { lifecycle: 'failed' }))).toMatchObject({ glyph: '✕', word: 'failed', tone: 'red' });
  });

  it('done only applies to a running idle agent', () => {
    expect(agentState(seatView('a', { activity: 'working' }), true).key).toBe('working');
    expect(agentState(seatView('a', { lifecycle: 'stopped' }), true).key).toBe('stopped');
  });

  it('sorts blocked agents first and keeps the rest in order', () => {
    const keys = ['idle', 'working', 'needs-you', 'done', 'failed', 'idle'] as const;
    const sorted = sortAgents(keys.map((k, i) => ({ k, i })), (x) => x.k);
    expect(sorted.map((x) => x.i)).toEqual([2, 4, 0, 1, 3, 5]);
  });
});

describe('activity', () => {
  it('notes a spawn the team\'s own limit refused in the latest turn, and only that', () => {
    const refused = (seq: number, reason: string, tool = 'spawn_agent') => ev(seq, { type: 'report-rejected', tool, reason, raw: {} });
    const limit = 'Team limit you set: 20 agents, children included';
    expect(activityOf([ev(1, { type: 'turn-started', turn_id: 't1' }), refused(2, limit)]).limitNote).toBe(limit);
    expect(activityOf([ev(1, { type: 'turn-started', turn_id: 't1' }), refused(2, 'Parent is stopping')]).limitNote).toBeUndefined();
    expect(activityOf([ev(1, { type: 'turn-started', turn_id: 't1' }), refused(2, limit, 'report')]).limitNote).toBeUndefined();
    expect(activityOf([refused(1, limit), ev(2, { type: 'turn-started', turn_id: 't2' })]).limitNote).toBeUndefined();
  });

  it('groups the latest turn tools by name, newest first, and finds the last reply and turn end', () => {
    const at = (e: ReturnType<typeof ev>, ms: number) => ({ ...e, at_ms: ms });
    const call = (seq: number, id: string, name: string, ms: number) =>
      at(ev(seq, { type: 'tool-called', turn_id: 't2', call_id: id, name, input: {}, parent_call_id: null }), ms);
    const result = (seq: number, id: string, ms: number, err = false) =>
      at(ev(seq, { type: 'tool-resulted', turn_id: 't2', call_id: id, output: '', is_error: err }), ms);
    const events = [
      ev(1, { type: 'turn-started', turn_id: 't1' }),
      call(2, 'old', 'Grep', 0),
      ev(3, { type: 'turn-ended', turn_id: 't1', status: 'completed' }),
      ev(4, { type: 'turn-started', turn_id: 't2' }),
      call(5, 'c1', 'Bash', 1000),
      result(6, 'c1', 1300),
      call(7, 'c2', 'Bash', 2000),
      result(8, 'c2', 2100),
      call(9, 'c3', 'Read', 2200),
      result(10, 'c3', 2210, true),
      ev(11, { type: 'agent-text', turn_id: 't2', text: 'looking' }),
      call(12, 'c4', 'Edit', 3000),
    ];
    const a = activityOf(events);
    expect(a.chips.map((c) => chipText(c, 3400))).toEqual(['⚒ edit 0.4s …', '⚒ read ✗', '⚒ bash ×2 ✓']);
    expect(a.lastText).toBe('looking');
    expect(a.lastTurnEndSeq).toBe(3);
  });

  it('keeps at most three chips', () => {
    const events = ['A', 'B', 'C', 'D'].map((n, i) => ev(i + 1, { type: 'tool-called', turn_id: 't', call_id: n, name: n, input: {}, parent_call_id: null }));
    expect(activityOf(events).chips.map((c) => c.name)).toEqual(['d', 'c', 'b']);
    expect(activityOf(events).toolCount).toBe(4);
  });
});

describe('words', () => {
  it('ages compactly, and unknown when missing', () => {
    expect(shortAge(null, 0)).toBe('unknown');
    expect(shortAge(0, 30_000)).toBe('now');
    expect(shortAge(0, 14 * 60_000)).toBe('14m');
    expect(shortAge(0, 3 * 3_600_000)).toBe('3h');
    expect(shortAge(0, 72 * 3_600_000)).toBe('3d');
  });

  it('says tokens unknown before any usage report', () => {
    expect(tokensText(seatView('a'))).toBe('tokens unknown');
    expect(tokensText(seatView('a', { tokens_in: 1000, tokens_out: 234 }))).toBe('1.2k tokens');
  });

  it('marks vendors', () => {
    expect(vendor('claude-code')).toMatchObject({ mark: '✳', name: 'Claude' });
    expect(vendor('codex')).toMatchObject({ mark: '◈', name: 'Codex' });
    expect(vendor('stub')).toMatchObject({ mark: '◇', name: 'Practice' });
  });

  it('never shows an agent claim as verified', () => {
    expect(tierOf('agent-reported')).toMatchObject({ glyph: '◇', word: 'reported' });
    expect(tierOf('protocol-observed')).toMatchObject({ glyph: '◆', word: 'observed', detail: 'protocol-observed' });
    expect(tierOf('captured-tool')).toMatchObject({ word: 'observed' });
    // What an agent says is its report, whatever channel carried it.
    expect(tierOf('protocol-observed', true)).toMatchObject({ glyph: '◇', word: 'reported', detail: 'protocol-observed' });
    expect(tierOf('verified', true)).toMatchObject({ word: 'reported' });
    expect(tierOf('heuristic')).toMatchObject({ glyph: '≈', word: 'heuristic' });
    expect(tierOf('verified')).toMatchObject({ glyph: '◆', word: 'verified' });
    expect(tierOf(null)).toMatchObject({ glyph: '∅', word: 'unknown' });
  });
});

describe('teamTokensText', () => {
  it('adds up the tokens every agent reported, and says when some have not', async () => {
    const { teamTokensText } = await import('./herdr');
    const s = (i: number | null, o: number | null) => ({ tokens_in: i, tokens_out: o });
    expect(teamTokensText([s(1_000_000, 200_000), s(300_000, 0)])).toBe('1.5M tokens so far');
    expect(teamTokensText([s(1_000, 0), s(null, null)])).toBe('at least 1.0k tokens so far (1 agent has not reported)');
    expect(teamTokensText([s(null, null)])).toBe('');
  });
});
