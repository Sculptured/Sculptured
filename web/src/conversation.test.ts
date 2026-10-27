import { describe, expect, it } from 'vitest';
import { conversationItems, HANDOFF_ACTOR, openTurn, parseHandoff, TEAM_START_ACTOR } from './conversation';
import { ev } from './test-fixtures';

describe('conversationItems', () => {
  it('merges tool results into their call and keeps order', () => {
    const items = conversationItems([
      ev(1, { type: 'message-queued', message_id: 'm1', intent: 'inform', text: 'tool:ls' }),
      ev(2, { type: 'tool-called', turn_id: 't1', call_id: 'c1', name: 'Bash', input: { command: 'ls' }, parent_call_id: null }),
      ev(3, { type: 'tool-resulted', turn_id: 't1', call_id: 'c1', output: 'a.txt', is_error: false }),
      ev(4, { type: 'agent-text', turn_id: 't1', text: 'done' }),
      ev(5, { type: 'turn-ended', turn_id: 't1', status: 'completed' }),
    ]);
    expect(items.map((i) => i.kind)).toEqual(['operator', 'tool', 'agent', 'turn-end']);
    const tool = items[1];
    expect(tool.kind === 'tool' && tool.output).toBe('a.txt');
  });

  it('says whether a result passed its checks, what failed, and whether it went back', () => {
    const fail = { rule_id: 'price', field: 'price', passed: false, actual: 7300, words: 'price is 7300, at most 5000' };
    const items = conversationItems([
      ev(1, { type: 'result-checked', turn_id: 't1', passed: false, results: [fail], bounced: true }),
      ev(2, { type: 'result-checked', turn_id: 't2', passed: false, results: [fail], bounced: false }),
      ev(3, { type: 'result-checked', turn_id: 't3', passed: true, results: [], bounced: false }),
    ]);
    expect(items.map((i) => i.kind === 'notice' && [i.text, !!i.alarm])).toEqual([
      ['✗ Its result did not pass its checks (price is 7300, at most 5000): sent back to it', true],
      ['✗ Its result did not pass its checks (price is 7300, at most 5000): not handed on; it waits for you', true],
      ['✓ Its result passed its checks and was handed on', false],
    ]);
  });

  it('moves message state forward only, and failed is terminal', () => {
    const consumedFirst = conversationItems([
      ev(1, { type: 'message-queued', message_id: 'm1', intent: 'instruct', text: 'hi' }),
      ev(2, { type: 'message-consumed', message_id: 'm1', turn_id: 't1' }),
      ev(3, { type: 'message-delivered', message_id: 'm1' }),
    ]);
    expect(consumedFirst[0]).toMatchObject({ kind: 'operator', state: 'consumed' });

    const failed = conversationItems([
      ev(1, { type: 'message-queued', message_id: 'm2', intent: 'inform', text: 'x' }),
      ev(2, { type: 'message-failed', message_id: 'm2', reason: "Sculptured's agent runner is not connected" }),
      ev(3, { type: 'message-delivered', message_id: 'm2' }),
    ]);
    expect(failed[0]).toMatchObject({ state: 'failed', reason: "Sculptured's agent runner is not connected" });
  });

  it('keeps unrecognized events and turns lifecycle changes into notices', () => {
    const items = conversationItems([
      ev(1, { type: 'unrecognized', raw: { type: 'mystery' } }),
      ev(2, { type: 'seat-lifecycle-changed', lifecycle: 'attention', reason: 'exited unexpectedly (code 3): boom' }),
      ev(3, { type: 'events-lost', count: 4 }),
    ]);
    expect(items[0]).toMatchObject({ kind: 'unrecognized', raw: { type: 'mystery' } });
    expect(items[1]).toMatchObject({ kind: 'notice', text: 'needs attention — exited unexpectedly (code 3): boom' });
    expect(items[2]).toMatchObject({ kind: 'notice', text: '4 events lost — history has a gap' });
  });

  it('merges interleaved calls by call id', () => {
    const items = conversationItems([
      ev(1, { type: 'tool-called', turn_id: 't', call_id: 'c1', name: 'A', input: {}, parent_call_id: null }),
      ev(2, { type: 'tool-called', turn_id: 't', call_id: 'c2', name: 'B', input: {}, parent_call_id: null }),
      ev(3, { type: 'tool-resulted', turn_id: 't', call_id: 'c2', output: 'out2', is_error: true }),
      ev(4, { type: 'tool-resulted', turn_id: 't', call_id: 'c1', output: 'out1', is_error: false }),
    ]);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ name: 'A', output: 'out1', isError: false });
    expect(items[1]).toMatchObject({ name: 'B', output: 'out2', isError: true });
  });

  it('includes the signal in tenure-ended notices and the provenance on agent text', () => {
    const items = conversationItems([
      ev(1, { type: 'tenure-ended', tenure: 2, exit_code: null, signal: 9, stderr_tail: '' }),
      ev(2, { type: 'agent-text', turn_id: null, text: 'x' }),
    ]);
    expect(items[0]).toMatchObject({ text: 'Session #2 ended · exit unknown · signal 9' });
    expect(items[1]).toMatchObject({ provenance: 'protocol-observed' });
  });

  // I6: a message the process never read must not stay queued or delivered forever.
  it('fails queued and delivered messages when the tenure ends, with the reason', () => {
    const items = conversationItems([
      ev(1, { type: 'message-queued', message_id: 'q', intent: 'inform', text: 'one' }),
      ev(2, { type: 'message-queued', message_id: 'd', intent: 'inform', text: 'two' }),
      ev(3, { type: 'message-delivered', message_id: 'd' }),
      ev(4, { type: 'message-queued', message_id: 'c', intent: 'inform', text: 'three' }),
      ev(5, { type: 'message-consumed', message_id: 'c', turn_id: 't1' }),
      ev(6, { type: 'tenure-ended', tenure: 1, exit_code: 1, signal: null, stderr_tail: '' }),
      ev(7, { type: 'message-queued', message_id: 'after', intent: 'inform', text: 'four' }),
    ]);
    const ops = items.filter((i) => i.kind === 'operator');
    expect(ops.map((m) => [m.messageId, m.state, m.reason])).toEqual([
      ['q', 'failed', 'the agent stopped before reading this message'],
      ['d', 'failed', 'the agent stopped before reading this message'],
      ['c', 'consumed', undefined],
      ['after', 'queued', undefined],
    ]);
  });

  it('marks the team Job, sent by the system at start, and keeps a failed one failed with its reason', () => {
    const items = conversationItems([
      { ...ev(1, { type: 'message-queued', message_id: 'j', intent: 'inform', text: 'Build it' }), actor: { kind: 'system', id: TEAM_START_ACTOR } },
      ev(2, { type: 'message-queued', message_id: 'h', intent: 'inform', text: 'hi' }),
      { ...ev(3, { type: 'message-queued', message_id: 'k', intent: 'inform', text: 'Review it' }), actor: { kind: 'system', id: TEAM_START_ACTOR } },
      ev(4, { type: 'message-failed', message_id: 'k', reason: 'the agent never started' }),
    ]);
    expect(items[0]).toMatchObject({ atStart: true, state: 'queued' });
    expect(items[1]).toMatchObject({ atStart: false });
    expect(items[2]).toMatchObject({ atStart: true, state: 'failed', reason: 'the agent never started' });
  });

  it('marks handoff messages and splits them into Job and upstream results', () => {
    const text = "Handed over by Planner, Researcher\nBuild the login page.\n--- Planner's result ---\n1. Add a form\n--- Researcher's result ---\nUse OAuth.";
    const items = conversationItems([
      { ...ev(1, { type: 'message-queued', message_id: 'h', intent: 'inform', text }), actor: { kind: 'system', id: HANDOFF_ACTOR } },
      {...ev(2, { type: 'message-queued', message_id: 'o', intent: 'inform', text: 'Handed over by me' }),actor:{kind:'human',id:'operator'}},
    ]);
    expect(items[0]).toMatchObject({ origin: 'handoff', atStart: false });
    expect(items[0].kind === 'operator' && items[0].handoff).toEqual({
      kind: 'handed-over',
      title: 'Handed over by Planner, Researcher',
      from: ['Planner', 'Researcher'],
      job: 'Build the login page.',
      results: [
        { name: 'Planner', text: '1. Add a form' },
        { name: 'Researcher', text: 'Use OAuth.' },
      ],
    });
    // Only the handoff actor makes a handoff card; an operator typing the same words does not.
    expect(items[1]).toMatchObject({ origin: 'operator', handoff: undefined });
  });

  it('parses a later handoff and leaves unknown text unparsed', () => {
    expect(parseHandoff("More work from Builder\n--- Builder's result ---\nDone.")).toMatchObject({
      kind: 'more-work',
      from: ['Builder'],
      job: '',
      results: [{ name: 'Builder', text: 'Done.' }],
    });
    expect(parseHandoff('Please review this')).toBeNull();
  });

  it('gives each turn end its duration and tokens, or null when unknown', () => {
    const at = (seq: number, ms: number, body: Parameters<typeof ev>[1]) => ({ ...ev(seq, body), at_ms: ms });
    const items = conversationItems([
      at(1, 1000, { type: 'turn-started', turn_id: 't1' }),
      at(2, 1500, { type: 'message-queued', message_id: 'm', intent: 'inform', text: 'hi' }),
      at(3, 1600, { type: 'message-consumed', message_id: 'm', turn_id: 't1' }),
      at(4, 2000, { type: 'usage-reported', turn_id: 't1', input_tokens: 10, output_tokens: 5, cache_read_tokens: 2, cache_write_tokens: 1 }),
      at(5, 4500, { type: 'turn-ended', turn_id: 't1', status: 'completed' }),
      at(6, 5000, { type: 'turn-ended', turn_id: 't2', status: 'failed' }),
    ]);
    expect(items[0]).toMatchObject({ kind: 'operator', turnId: 't1' });
    expect(items[1]).toMatchObject({ kind: 'turn-end', durationMs: 3500, tokens: { input: 13, output: 5 } });
    expect(items[2]).toMatchObject({ kind: 'turn-end', durationMs: null, tokens: null });
  });

  it('knows which turn is still open; a session end closes it', () => {
    const started = ev(1, { type: 'turn-started', turn_id: 't1' });
    expect(openTurn([started])).toBe('t1');
    expect(openTurn([started, ev(2, { type: 'turn-ended', turn_id: 't1', status: 'completed' })])).toBeNull();
    expect(openTurn([started, ev(2, { type: 'tenure-ended', tenure: 1, exit_code: 3, signal: null, stderr_tail: '' })])).toBeNull();
  });
});


it('attributes captured peer messages to the agent rather than You',()=>{
 const message={...ev(1,{type:'message-queued',message_id:'peer',intent:'inform',text:'Please review'}),actor:{kind:'seat' as const,id:'dev.helper@r'}};
 expect(conversationItems([message])[0]).toMatchObject({origin:'agent',sender:'dev.helper@r'});
});

describe('protections', () => {
  it('show a refusal and a fresh continue as plain notices with the raw reason kept', () => {
    const items = conversationItems([
      ev(1, { type: 'protection-applied', call_id: 'c1', reason: 'Sculptured protection: big.csv is 293 KB, too large to read whole.' }),
      ev(2, { type: 'context-ceiling-reached', used_tokens: 61234, ceiling: 60000 }),
    ]);
    const texts = items.map((i) => (i.kind === 'notice' ? i.text : ''));
    expect(texts[0]).toBe('Sculptured refused this call: big.csv is 293 KB, too large to read whole.');
    expect(texts[1]).toBe('Context 61k passed the 60k limit: this agent writes its notes and continues in a fresh conversation');
  });
});
