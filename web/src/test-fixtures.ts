import type { DraftAgent } from './generated/DraftAgent';
import type { DraftConnection } from './generated/DraftConnection';
import type { TeamDraft } from './generated/TeamDraft';
import type { TeamRecord } from './generated/TeamRecord';
import type { TemplateInfo } from './generated/TemplateInfo';
import type { CompiledRig } from './generated/CompiledRig';
import type { CompiledSeat } from './generated/CompiledSeat';
import type { SeatView } from './generated/SeatView';
import type { RigSnapshot } from './generated/RigSnapshot';
import type { EventRecord } from './generated/EventRecord';
import type { EventBody } from './generated/EventBody';
import type { EventSourceLike, OpenStream } from './useRigStream';

/** In-memory EventSource for hook and component tests. Never touches the network. */
export class FakeEventSource implements EventSourceLike {
  static instances: FakeEventSource[] = [];
  static reset() {
    FakeEventSource.instances = [];
  }
  listeners: Record<string, ((e: MessageEvent) => void)[]> = {};
  onerror: ((e: Event) => void) | null = null;
  closed = false;
  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }
  addEventListener(type: string, fn: (e: MessageEvent) => void) {
    (this.listeners[type] ??= []).push(fn);
  }
  close() {
    this.closed = true;
  }
  emit(type: string, data: unknown) {
    for (const fn of this.listeners[type] ?? []) fn(new MessageEvent(type, { data: JSON.stringify(data) }));
  }
  fail() {
    this.onerror?.(new Event('error'));
  }
}

/** Module-level so its identity is stable across renders. */
export const openFake: OpenStream = (url) => new FakeEventSource(url);

export function seatSpec(member: string): CompiledSeat {
  return {
    id: `dev.${member}@r`,
    pod: 'dev',
    member,
    runtime: 'claude-code',
    cwd: '.',
    agent_ref: `local:agents/${member}`,
    profile: 'default',
    label: null,
    role: null,
    model: null,
    restore_policy: 'resume_if_possible',
    permission_policy: null,
  };
}

/** Rig `r`: pod `dev` with seats a and b; a delegates_to b (enforced), b can_observe a (intent). */
export function compiledRig(): CompiledRig {
  return {
    name: 'r',
    summary: null,
    spec_hash: 'sha256:test',
    pods: [{ id: 'dev', label: 'Dev' }],
    seats: [seatSpec('a'), seatSpec('b')],
    edges: [
      { kind: 'delegates_to', from: 'dev.a@r', to: 'dev.b@r', enforced: true },
      { kind: 'can_observe', from: 'dev.b@r', to: 'dev.a@r', enforced: false },
    ],
    launch_layers: [['dev.a@r'], ['dev.b@r']],
    diagnostics: [],
  };
}

export function seatView(member: string, over: Partial<SeatView> = {}): SeatView {
  return {
    id: `dev.${member}@r`,
    rig: 'r',
    pod: 'dev',
    member,
    runtime: 'claude-code',
    label: null,
    host: 'local',
    lifecycle: 'running',
    lifecycle_reason: null,
    activity: 'idle',
    tenure: 1,
    native_session_id: `session-dev.${member}@r`,
    caps: {
      interrupt: true,
      questions: false,
      resume: true,
      fork: false,
      usage: true,
      mid_turn_input: false,
      subagent_events: false,
    },
    current_turn: null,
    current_tool: null,
    last_event_ms: null,
    tokens_in: null,
    tokens_out: null,
    lifecycle_provenance: 'protocol-observed',
    open_questions: 0,
    risk_count: 0,
    restore_offer: null,
    cwd: null,
    limit_hit: null,
    model_requested: null,
    model_observed: null,
    stalled: null,
    ...over,
  };
}

export function snapshot(over: Partial<RigSnapshot> = {}): RigSnapshot {
  return { rig: compiledRig(), desired: 'running', seats: [seatView('a'), seatView('b')], last_seq: 10, ...over };
}

export function ev(seq: number, body: EventBody, seat = 'dev.a@r'): EventRecord {
  return {
    seq,
    host: 'local',
    host_seq: seq,
    at_ms: seq,
    actor: { kind: 'system', id: 'hostd' },
    subject: { kind: 'seat', id: seat },
    provenance: 'protocol-observed',
    body,
  };
}

export function draftAgent(id: string, over: Partial<DraftAgent> = {}): DraftAgent {
  return {
    id,
    group_id: null,
    name: id[0].toUpperCase() + id.slice(1),
    kind: 'claude',
    job: '',
    folder: null,
    x: 40,
    y: 40,
    advanced: { model: null, permission: 'unrestricted', resume: 'resume-if-possible' },
    ...over,
  };
}

/** Team `t`: Builder (40,40) hands work to Reviewer (300,40), both Claude agents. */
export function teamDraft(over: Partial<TeamDraft> = {}): TeamDraft {
  return {
    id: 't',
    name: 'My team',
    groups: [],
    agents: [draftAgent('builder', { x: 40, y: 40 }), draftAgent('reviewer', { x: 300, y: 40 })],
    connections: [{ id: 'c1', from: 'builder', to: 'reviewer', kind: 'hands-work-to' }],
    revision: 1,
    ...over,
  };
}

export function teamRecord(over: Partial<TeamRecord> = {}): TeamRecord {
  return { draft: teamDraft(), started_revision: null, live: false, issues: [], ...over };
}

export function templateInfos(): TemplateInfo[] {
  const preview = (agents: DraftAgent[], connections: DraftConnection[] = []): TeamDraft => ({
    id: 'preview',
    name: 'preview',
    groups: [],
    agents,
    connections,
    revision: 0,
  });
  const hands = (from: string, to: string): DraftConnection => ({ id: `c-${from}-${to}`, from, to, kind: 'hands-work-to' });
  return [
    { id: 'solo', title: 'Solo agent', summary: 'One Claude agent.', default_name: 'Solo agent', preview: preview([draftAgent('claude', { name: 'Claude', kind: 'claude' })]) },
    {
      id: 'builder-reviewer',
      title: 'Builder → Reviewer',
      summary: 'Claude builds, then hands the work to Codex to review.',
      default_name: 'Builder and Reviewer',
      preview: preview(
        [draftAgent('builder', { name: 'Builder', kind: 'claude' }), draftAgent('reviewer', { name: 'Reviewer', kind: 'codex', x: 300 })],
        [hands('builder', 'reviewer')],
      ),
    },
    {
      id: 'planner-builders',
      title: 'Planner → 2 Builders',
      summary: 'One planner hands work to two builders.',
      default_name: 'Planner and builders',
      preview: preview(
        [
          draftAgent('planner', { name: 'Planner', kind: 'claude', y: 160 }),
          draftAgent('builder-1', { name: 'Builder 1', kind: 'claude', x: 300 }),
          draftAgent('builder-2', { name: 'Builder 2', kind: 'claude', x: 300, y: 280 }),
        ],
        [hands('planner', 'builder-1'), hands('planner', 'builder-2')],
      ),
    },
    { id: 'blank', title: 'Blank canvas', summary: 'Start empty and drag agents in.', default_name: 'New team', preview: preview([]) },
  ];
}
