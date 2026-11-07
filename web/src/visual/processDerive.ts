import { splitTeamMessage } from '../teamMessage';
import { originOf, parseHandoff, STOPPED_BEFORE_READING } from '../conversation';
import type { EventRecord } from '../generated/EventRecord';
import { checkOf, checkOutcome, editOf, TOOL_KINDS, toolKind, toolPath, toolSummary, type ToolKind } from './tools';
import type { Check, Claim, FileEntry, Phase, Process, Step, Trigger } from './process';

const firstLine = (s: string) => s.split('\n').find((l) => l.trim())?.trim() ?? '';

const CLAIM =
  /\b(?:all\s+)?(?:tests?|checks?|builds?|specs?|suite)\b[^.\n]{0,40}?\b(?:pass(?:es|ed|ing)?|succeed(?:s|ed)?|green|work(?:s|ing)?)\b|\b(?:done|completed?|finished|fixed|implemented|working now)\b/i;

/** The first sentence of a reply that claims success, verbatim. */
export function claimIn(text: string): string | null {
  for (const s of text.split(/(?<=[.!?])\s+|\n+/)) {
    const t = s.trim();
    if (t && CLAIM.test(t)) return t.length > 200 ? `${t.slice(0, 200)}…` : t;
  }
  return null;
}

type Body<K extends EventRecord['body']['type']> = Extract<EventRecord['body'], { type: K }>;

/** Folds events into phases (one per turn), their steps and replies, and the messages that triggered them. */
class ProcessBuilder {
  phases: Phase[] = [];
  messages = new Map<string, Trigger>();
  consumed = new Set<string>();
  replies = new Map<Phase, { key: string; text: string }>();
  sessions = 0;
  lastActivityMs: number | null = null;
  private byTurn = new Map<string, Phase>();
  private steps = new Map<string, Step>();
  private calledAt = new Map<string, number>();
  private current: Phase | null = null;

  event(e: EventRecord) {
    this.lastActivityMs = this.lastActivityMs === null ? e.at_ms : Math.max(this.lastActivityMs, e.at_ms);
    const handle = this.handlers[e.body.type];
    (handle as ((this: ProcessBuilder, e: EventRecord, b: EventRecord['body']) => void) | undefined)?.call(this, e, e.body);
  }

  private handlers: Partial<Record<EventRecord['body']['type'], (this: ProcessBuilder, e: EventRecord, b: never) => void>> = {
    'tenure-started': () => { this.sessions++; },
    'turn-started': (e, b: Body<'turn-started'>) => { if (!this.byTurn.has(b.turn_id)) this.open(b.turn_id, e); },
    'message-queued': this.queued,
    'message-delivered': (_e, b: Body<'message-delivered'>) => this.delivered(b),
    'message-failed': (_e, b: Body<'message-failed'>) => this.failed(b),
    'message-consumed': this.consumedBy,
    'tool-called': this.toolCalled,
    'tool-resulted': this.toolResulted,
    'agent-text': this.agentText,
    'usage-reported': (_e, b: Body<'usage-reported'>) => this.usage(b),
    'turn-ended': this.turnEnded,
    'tenure-ended': (e) => this.tenureEnded(e),
  };

  private open(turnId: string | null, e: EventRecord): Phase {
    const p: Phase = {
      id: turnId ?? `p-${e.seq}`, n: this.phases.length + 1, turnId, triggers: [], steps: [], counts: {}, failedSteps: 0, reply: null, explanations: [],
      status: 'running', startMs: e.at_ms, endMs: null, tokens: null, anchorKey: null, endKey: null,
    };
    this.phases.push(p);
    if (turnId) this.byTurn.set(turnId, p);
    this.current = p;
    return p;
  }

  private phaseFor(turnId: string | null, e: EventRecord): Phase {
    if (turnId) return this.byTurn.get(turnId) ?? this.open(turnId, e);
    return this.current ?? this.open(null, e);
  }

  private anchor(p: Phase, key: string) {
    if (p.anchorKey === null) p.anchorKey = key;
  }

  private queued(e: EventRecord, b: Body<'message-queued'>) {
    const kind = originOf(e);
    this.messages.set(b.message_id, {
      kind, key: `m-${b.message_id}`, firstLine: firstLine(splitTeamMessage(b.text).text), text: b.text,
      from: kind === 'handoff' ? (parseHandoff(b.text)?.from ?? []) : e.actor.kind === 'seat' ? [e.actor.id] : [],
      state: 'queued',
    });
  }

  private delivered(b: Body<'message-delivered'>) {
    const m = this.messages.get(b.message_id);
    if (m && m.state === 'queued') m.state = 'delivered';
  }

  private failed(b: Body<'message-failed'>) {
    const m = this.messages.get(b.message_id);
    if (m && m.state !== 'consumed') {
      m.state = 'failed';
      m.reason = b.reason;
    }
  }

  private consumedBy(e: EventRecord, b: Body<'message-consumed'>) {
    const m = this.messages.get(b.message_id);
    const p = this.phaseFor(b.turn_id, e);
    if (m && m.state !== 'failed') {
      m.state = 'consumed';
      this.consumed.add(b.message_id);
      p.triggers.push(m);
      // The message that started the turn comes before the turn's own events.
      p.anchorKey = p.triggers[0].key;
    }
  }

  private toolCalled(e: EventRecord, b: Body<'tool-called'>) {
    const p = this.phaseFor(b.turn_id, e);
    const s: Step = {
      key: `t-${b.call_id}-${e.seq}`, callId: b.call_id, name: b.name, kind: toolKind(b.name), summary: toolSummary(b.name, b.input),
      status: 'running', ms: null, helper: b.parent_call_id !== null, input: b.input,
    };
    this.calledAt.set(b.call_id, e.at_ms);
    this.steps.set(b.call_id, s);
    p.steps.push(s);
    p.counts[s.kind] = (p.counts[s.kind] ?? 0) + 1;
    this.anchor(p, s.key);
  }

  private toolResulted(e: EventRecord, b: Body<'tool-resulted'>) {
    const s = this.steps.get(b.call_id);
    if (!s) return;
    s.status = b.is_error ? 'error' : 'ok';
    s.output = b.output;
    s.isError = b.is_error;
    s.ms = Math.max(0, e.at_ms - (this.calledAt.get(b.call_id) ?? e.at_ms));
  }

  private agentText(e: EventRecord, b: Body<'agent-text'>) {
    const p = this.phaseFor(b.turn_id, e);
    const key = `a-${e.seq}`;
    if (b.text.trim()) {
      p.reply = { key, firstLine: firstLine(b.text), text: b.text, provenance: e.provenance };
      p.explanations!.push({ key, text: b.text, provenance: e.provenance });
      this.replies.set(p, { key, text: b.text });
    }
    this.anchor(p, key);
  }

  private usage(b: Body<'usage-reported'>) {
    const p = b.turn_id ? this.byTurn.get(b.turn_id) : this.current;
    if (!p) return;
    const t = p.tokens ?? { input: 0, output: 0 };
    t.input += b.input_tokens + b.cache_read_tokens + b.cache_write_tokens;
    t.output += b.output_tokens;
    p.tokens = t;
  }

  private turnEnded(e: EventRecord, b: Body<'turn-ended'>) {
    const p = this.phaseFor(b.turn_id, e);
    p.status = b.status;
    p.endMs = e.at_ms;
    p.endKey = `e-${e.seq}`;
    this.anchor(p, p.endKey);
    if (this.current === p) this.current = null;
  }

  private tenureEnded(e: EventRecord) {
    if (this.current) {
      this.current.status = 'cut-off';
      this.current.endMs = e.at_ms;
    }
    this.current = null;
    for (const m of this.messages.values()) {
      if (m.state === 'queued' || m.state === 'delivered') {
        m.state = 'failed';
        m.reason = STOPPED_BEFORE_READING;
      }
    }
  }
}

/** Each phase's goal: the latest job or operator request seen so far. */
function setGoals(phases: Phase[]) {
  let goal: Trigger | undefined;
  for (const p of phases) {
    goal = p.triggers.find(t => t.kind === 'job' || t.kind === 'operator') ?? goal;
    p.goal = goal;
  }
  for (const p of phases) p.failedSteps = p.steps.filter((s) => s.status === 'error').length;
}

/** Files, in the order they were first touched, and the checks that were run. */
function filesAndChecks(phases: Phase[]) {
  const files = new Map<string, FileEntry>();
  const checks: Check[] = [];
  for (const p of phases) {
    for (const s of p.steps) {
      if (s.kind === 'read') readFile(files, p, s);
      else if (s.kind === 'edit') editFile(files, p, s);
      else if (s.kind === 'bash') {
        const kind = checkOf(s.summary.replace(/ …$/, ''));
        if (kind) checks.push({ key: s.key, phase: p.n, command: s.summary, kind, outcome: checkOutcome(s.output, s.isError), helper: s.helper });
      }
    }
  }
  return { files: [...files.values()], checks };
}

function readFile(files: Map<string, FileEntry>, p: Phase, s: Step) {
  const path = toolPath(s.input);
  if (!path) return;
  const f = files.get(path) ?? { path, reads: 0, changes: [], last: { action: 'read' as const, phase: p.n, key: s.key } };
  f.reads++;
  f.last = { action: 'read', phase: p.n, key: s.key };
  files.set(path, f);
}

function editFile(files: Map<string, FileEntry>, p: Phase, s: Step) {
  const edit = editOf(s.name, s.input, s.output);
  if (!edit) return;
  const f = files.get(edit.path) ?? { path: edit.path, reads: 0, changes: [], last: { action: edit.action, phase: p.n, key: s.key } };
  f.changes.push({ key: s.key, phase: p.n, action: edit.action, hunks: edit.hunks, status: s.status });
  f.last = { action: edit.action, phase: p.n, key: s.key };
  files.set(edit.path, f);
}

function claimsOf(phases: Phase[], replies: Map<Phase, { key: string; text: string }>): Claim[] {
  const claims: Claim[] = [];
  for (const p of phases) {
    const r = replies.get(p);
    const text = r && claimIn(r.text);
    if (r && text) claims.push({ key: r.key, phase: p.n, text });
  }
  return claims;
}

function summarizeProcess(b: ProcessBuilder, files: FileEntry[], checks: Check[]): Process['summary'] {
  const byKind = Object.fromEntries(TOOL_KINDS.map((k) => [k, 0])) as Record<ToolKind, number>;
  let failedSteps = 0;
  for (const p of b.phases) {
    for (const s of p.steps) {
      byKind[s.kind]++;
      if (s.status === 'error') failedSteps++;
    }
  }
  return {
    turns: b.phases.filter((p) => p.turnId !== null).length,
    toolCalls: b.phases.reduce((n, p) => n + p.steps.length, 0),
    byKind,
    filesChanged: files.filter((f) => f.changes.length > 0).length,
    filesRead: files.filter((f) => f.reads > 0).length,
    commands: byKind.bash,
    checks: checks.length,
    checksWithError: checks.filter((c) => c.outcome.status === 'error').length,
    failedSteps,
    lastActivityMs: b.lastActivityMs,
    sessions: b.sessions,
  };
}

export function deriveProcess(events: EventRecord[]): Process {
  const b = new ProcessBuilder();
  for (const e of events) b.event(e);
  setGoals(b.phases);
  const { files, checks } = filesAndChecks(b.phases);
  return {
    phases: b.phases,
    files,
    checks,
    claims: claimsOf(b.phases, b.replies),
    waiting: [...b.messages.entries()].filter(([id]) => !b.consumed.has(id)).map(([, m]) => m),
    summary: summarizeProcess(b, files, checks),
  };
}
