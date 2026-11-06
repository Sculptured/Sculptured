import { describe, expect, it } from 'vitest';
import type { EventBody } from '../../generated/EventBody';
import { draftAgent, ev, teamDraft } from '../../test-fixtures';
import { deploymentPlant, memberOf } from './deploymentPlant';

const called = (seq: number, call: string, name: string, input: unknown = {}) => ev(seq * 1000, { type: 'tool-called', turn_id: 't', call_id: call, name, input: input as never, parent_call_id: null });
const result = (seq: number, call: string, output = 'ok', is_error = false) => ev(seq * 1000, { type: 'tool-resulted', turn_id: 't', call_id: call, output, is_error });
const cls = (seq: number, call: string, c: Extract<EventBody, { type: 'action-interpreted' }>['class']) => ev(seq * 1000, { type: 'action-interpreted', call_id: call, class: c, paths: [], risks: [] });

describe('a deployment as a plant', () => {
  it("places each call on the system its tool belongs to, with Sculptured's effect, duration and outcome", () => {
    const draft = teamDraft({ components: [{ id: 'erp', name: 'ERPNext MCP', description: '', kind: 'mcp', program: 'erp-mcp', args: [] } as never] });
    const p = deploymentPlant([
      called(1, 'c1', 'Read'), cls(1, 'c1', { kind: 'file-read' }), result(2, 'c1'),
      called(3, 'c2', 'mcp__erp__create_order'), result(5, 'c2', 'blocked by safety rule: no orders over 10k', true),
      called(6, 'c3', 'mcp__rig__send_agent_message'), result(6, 'c3'),
      called(7, 'c4', 'mcp__rig__propose_change'),
      called(8, 'c5', 'Bash'), cls(8, 'c5', { kind: 'shell-other' }), result(9, 'c5', 'exit 1', true),
      called(10, 'c6', 'mcp__other__x'),
    ], draft, []);
    expect(p.actions.map((a) => [a.system, a.tool, a.effect, a.outcome, a.ms ?? null])).toEqual([
      ['files', 'Read', 'read', 'ok', 1000],
      ['mcp:erp', 'create_order', 'unknown', 'refused', 2000],
      ['team', 'send_agent_message', 'write', 'ok', 0],
      ['approvals', 'propose_change', 'write', 'running', null],
      ['shell', 'Bash', 'unknown', 'error', 1000],
      ['mcp:other', 'x', 'unknown', 'running', null],
    ]);
    expect(p.systems.find((s) => s.id === 'mcp:erp')).toMatchObject({ kind: 'mcp', name: 'ERPNext MCP', basis: 'sculptured' });
    expect(p.systems.find((s) => s.id === 'mcp:other')).toMatchObject({ kind: 'mcp', name: 'other', basis: 'unknown' });
  });

  it('a permission question becomes a permit on the call it allowed, with who granted it', () => {
    const p = deploymentPlant([
      ev(1000, { type: 'question-raised', question_id: 'q1', source: 'native-permission', prompt: { header: null, text: 'Run?', options: [], allow_other: false, tool: 'Bash', tool_input: null }, turn_id: 't' }),
      ev(2000, { type: 'question-answered', question_id: 'q1', answer: { kind: 'allow' }, late: false }),
      called(3, 'c1', 'Bash'), result(4, 'c1'),
    ], undefined, []);
    expect(p.permits).toEqual([{ id: 'q1', at: 1000, agent: 'dev.a@r', title: 'use Bash', state: 'granted', by: 'you' }]);
    expect(p.actions[0]!.permit).toBe('q1');
    expect(p.systems.map((s) => s.id)).toContain('approvals');
  });

  it("a proposed change is a permit decided by the team's rules or a person", () => {
    const team = (seq: number, body: EventBody) => ({ ...ev(seq, body), subject: { kind: 'rig' as const, id: 'r' }, actor: { kind: 'seat' as const, id: 'dev.a@r' } });
    const p = deploymentPlant([
      team(1, { type: 'change-proposed', proposal_id: 'p1', action: 'create_po', system: 'ERP', summary: 'Order 40 vials', payload: {}, reason: null }),
      team(2, { type: 'change-decided', proposal_id: 'p1', approved: true, by: 'mandate', note: null }),
    ], undefined, []);
    expect(p.permits).toEqual([{ id: 'p1', at: 1, agent: 'dev.a@r', title: 'Order 40 vials', state: 'granted', by: "the team's rules", system: 'ERP' }]);
  });

  it("an agent's rights come from its design: what it decides, whether it asks first, the team's rules", () => {
    const draft = teamDraft({
      agents: [draftAgent('a', { advanced: { ...draftAgent('a').advanced, permission: 'restricted', authority: { domains: ['batch release'] } } })],
      mandate: { rules: [{ id: 'r1', action: '*', field: 'amount', test: { kind: 'max', value: 10 } as never }] },
    });
    const p = deploymentPlant([], draft, ['team.a@r', 'team.b@r']);
    expect(p.rights['team.a@r']).toEqual(['decides: batch release', 'asks before using tools', 'changes it proposes are checked against 1 rule']);
    expect(p.rights['team.b@r']).toEqual([]);
    expect(memberOf('grp.a@r')).toBe('a');
  });

  it('usage: turn reports give tokens and cost; context reports give context size', () => {
    const p = deploymentPlant([
      ev(1, { type: 'usage-reported', turn_id: 't', input_tokens: 10, output_tokens: 5, cache_read_tokens: 100, cache_write_tokens: 0, cost: { usd: 0.2, source: 'api-equivalent' as never } }),
      ev(2, { type: 'usage-reported', turn_id: 't', input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, in_turn: true }),
      ev(3, { type: 'context-reported', turn_id: 't', used_tokens: 5000, window_tokens: null, basis: 'reported' as never }),
    ], undefined, []);
    expect(p.usage).toEqual([{ at: 1, agent: 'dev.a@r', tokens: 115, costUsd: 0.2 }, { at: 3, agent: 'dev.a@r', context: 5000 }]);
  });
});
