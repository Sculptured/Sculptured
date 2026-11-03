import { vi } from 'vitest';
import { FakeEventSource } from '../test-fixtures';
import type { BranchDetail, Event, EventKind, RunSummary } from '../tests/types';
import { FakeChannel } from '../workspace/testkit';

/**
 * Test fixtures for simulated runs: run r1, branch b1 (Triage then Resolver, task 27) in a world of
 * retail, plant and a Sculptured approvals part (desk), and its fork b2. `/api/arena/*` is answered
 * by `fetch` stubbed here; tests change `arenaFake` to vary the answers.
 */

export const at = (s: number) => new Date(Date.parse('2026-11-02T10:00:00Z') + s * 1000).toISOString();
export const ev = (seq: number, t: number, agent: string | null, kind: EventKind, payload: unknown = {}, branch = 'b1'): Event =>
  ({ branch_id: branch, seq, at: at(t), agent, kind, payload });

const change = (id: string, summary: string, decision: unknown = null) => ({ id, action: 'refund', system: 'shop', summary, payload: {}, reason: 'Boots arrived damaged', decision });
const WORLD = [{ part: 'retail', integration: 'mongo' }, { part: 'plant', integration: 'counter' }, { part: 'desk', integration: 'sculptured' }];

export function events(): Event[] {
  return [
    ev(0, 0, null, 'run_start', { task_id: '27', world: WORLD, fingerprints: { retail: { server_version: '16.4' } } }),
    ev(1, 0, 'triage', 'agent_start'),
    ev(2, 4, 'triage', 'tool_call', { name: 'get_order_details', args: { order_id: '#W1' }, result: {}, error: null, part: 'retail', reads: ['orders/#W1'], writes: [] }),
    ev(3, 5, 'triage', 'tool_call', { name: 'propose_change', args: {}, result: change('c1', 'Refund order #W1'), error: null, part: 'desk', reads: [], writes: ['change/c1'], saves: { desk: 'd1' } }),
    ev(4, 6, 'triage', 'handoff', { note: 'Swap the boots.' }),
    ev(5, 7, 'triage', 'agent_end', { ok: true, error: null, tokens: 100, cost_usd: 0.05, cost_source: 'api_equivalent_estimate', seconds: 7 }),
    ev(6, 7, 'resolver', 'agent_start'),
    ev(7, 15, 'resolver', 'tool_call', { name: 'exchange_delivered_order_items', args: {}, result: {}, error: null, part: 'retail', reads: [], writes: ['orders/#W1'], snapshot: 's1', saves: { plant: 'p1', retail: 's1' } }),
    ev(8, 16, 'resolver', 'tool_call', { name: 'propose_change', args: {}, result: change('c2', 'Refund shipping'), error: null, part: 'desk', reads: [], writes: ['change/c2'], saves: { desk: 'd2' } }),
    ev(9, 17, 'resolver', 'tool_call', { name: 'decide_change', args: {}, result: change('c2', 'Refund shipping', { approved: true, by: 'person', note: null }), error: null, part: 'desk', reads: [], writes: ['change/c2'], saves: { desk: 'd3' } }),
    ev(10, 18, 'resolver', 'agent_end', { ok: true, error: null, tokens: 200, cost_usd: null, cost_source: null, seconds: 11 }),
    ev(11, 19, null, 'check', { passed: true, gold_db: 'g', differing_rows: [] }),
  ];
}

const TEAM = {
  id: 't', name: 't',
  agents: [
    { id: 'triage', name: 'Triage', kind: 'claude_code' as const, model: null, prompt: 'Look things up.' },
    { id: 'resolver', name: 'Resolver', kind: 'codex' as const, model: null, prompt: 'Fix the order.' },
  ],
};

export function branchB1(): BranchDetail {
  return {
    id: 'b1', run_id: 'r1', parent_id: null, start_seq: null, changes: null, fork_note: null,
    status: 'passed', error: null, created_at: at(0), ended_at: at(20), team: TEAM, events: events(),
  };
}

/** A fork of b1 from step 7 whose plant part was restored approximately. */
export function branchB2(): BranchDetail {
  return {
    ...branchB1(), id: 'b2', parent_id: 'b1', start_seq: 7, fork_note: 'Rerun from step 7', created_at: at(30), ended_at: at(50),
    changes: { resolver: { prompt: 'Be careful.' } }, restore_mode: 'approximate',
    restored: [{ part: 'retail', how: 'save' }, { part: 'plant', how: 'approximate' }, { part: 'desk', how: 'save' }],
  };
}

/** Rows as saved after each step, by key. */
const ROWS: Record<number, Record<string, unknown>> = {
  3: { 'orders/#W1': { status: 'delivered' }, 'change/c1': { id: 'c1', decision: null } },
  6: { 'orders/#W1': { status: 'delivered' } },
  7: { 'orders/#W1': { status: 'exchange requested' }, 'change/c1': { id: 'c1', decision: null } },
  8: { 'orders/#W1': { status: 'exchange requested' }, 'change/c1': { id: 'c1', decision: null }, 'change/c2': { id: 'c2', decision: null } },
};

export const DOWN: [number, unknown] = [503, { error: { code: 'arena_unreachable', message: 'unreachable', raw: 'error sending request: connection refused' } }];
export const REFUSAL = "the part plant can't be restored exactly; choose an approximate restore";
export const PLANS: Record<string, [number, unknown]> = {
  exact: [200, { parts: [{ part: 'retail', how: 'save' }, { part: 'plant', how: 'replay', estimate_seconds: 40 }, { part: 'desk', how: 'save' }] }],
  approximate: [200, { parts: [{ part: 'retail', how: 'save' }, { part: 'plant', how: 'approximate' }, { part: 'desk', how: 'save' }] }],
};

export const arenaFake = {
  down: false,
  branches: {} as Record<string, BranchDetail>,
  runs: [] as RunSummary[],
  plans: { ...PLANS },
  calls: [] as { method: string; url: string; body?: unknown }[],
};

function rows(url: string): [number, unknown] {
  const u = new URL(url, 'http://x');
  const seq = Number(u.searchParams.get('seq'));
  const keys = (u.searchParams.get('keys') ?? '').split(',').filter(Boolean);
  return [200, { seq, rows: Object.fromEntries(keys.map((k) => [k, ROWS[seq]?.[k] ?? null])) }];
}

const COMPARE = {
  a: { status: 'passed', restore_mode: 'approximate', restored: [{ part: 'plant', how: 'approximate' }], seconds: 20, tokens: 250, cost_usd: null, cost_source: null },
  b: { status: 'passed', restore_mode: null, restored: [], seconds: 20, tokens: 300, cost_usd: null, cost_source: null },
  first_divergent_seq: 7, differing_rows: [{ key: 'orders/#W1', a: { status: 'exchanged' }, b: { status: 'exchange requested' } }],
};

function table(path: string): unknown {
  if (path === '/health') return { claude: true, codex: true, open_available: false };
  if (path === '/runs') return arenaFake.runs;
  if (path === '/world') {
    return { parts: [
      { name: 'retail', integration: 'mongo', version: '7.0', answered: true, restore: { exact: 'save', approximate: false }, uses_clock: false, tools: ['get_order_details'] },
      { name: 'plant', integration: 'counter', version: '1', answered: true, restore: { exact: 'replay', approximate: true }, uses_clock: true, tools: [] },
      { name: 'desk', integration: 'sculptured', version: '0.1', answered: true, restore: { exact: 'save', approximate: false }, uses_clock: false, tools: ['propose_change'] },
    ] };
  }
  if (path.startsWith('/compare?')) return COMPARE;
  const b = /^\/branches\/(\w+)$/.exec(path);
  return b ? arenaFake.branches[b[1]] : undefined;
}

function answer(method: string, url: string, body: unknown): [number, unknown] {
  if (arenaFake.down) return DOWN;
  const path = url.replace('/api/arena', '');
  const plan = /\/restore-plan\?at_seq=\d+&restore=(\w+)$/.exec(path);
  if (plan) return arenaFake.plans[plan[1]];
  if (method === 'POST' && /\/branches\/b1\/fork$/.test(path)) {
    arenaFake.branches.b2 = branchB2();
    arenaFake.runs = [{ id: 'r1', task_id: '27', created_at: at(0), branches: [branchB1(), branchB2()] }];
    return [200, { branch_id: 'b2', start_seq: (body as { at_seq: number }).at_seq, moved_from: null, reason: null }];
  }
  if (path.includes('/rows?')) return rows(path);
  const found = table(path);
  return found === undefined ? [404, { error: { code: 'arena_not_found', message: path } }] : [200, found];
}

/** Fresh answers: b1 alone in run r1, Arena up, every restore plan as PLANS. */
export function resetArena() {
  localStorage.clear();
  FakeChannel.open.clear();
  FakeEventSource.reset();
  vi.stubGlobal('BroadcastChannel', FakeChannel);
  vi.stubGlobal('EventSource', FakeEventSource);
  Object.assign(arenaFake, { down: false, branches: { b1: branchB1() }, plans: { ...PLANS }, calls: [] });
  arenaFake.runs = [{ id: 'r1', task_id: '27', created_at: at(0), branches: [branchB1()] }];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    arenaFake.calls.push({ method, url, body });
    if (!url.startsWith('/api/arena')) return new Response(JSON.stringify({ error: { code: 'not_found', message: url } }), { status: 404 });
    const [status, reply] = answer(method, url, body);
    return new Response(JSON.stringify(reply), { status });
  }));
}
