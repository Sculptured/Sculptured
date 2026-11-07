import type { EventBody } from '../generated/EventBody';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { SeatView } from '../generated/SeatView';
import type { TeamBudget } from '../generated/TeamBudget';
import { activityLines } from '../progress/sentences';
import { ACTIVITY, hasProcess } from '../status';
import { REFUSAL_WORDS } from './alarms';
import { eventWords } from './eventWords';
import { perMinute } from './rate';
import type { FeedItem } from './source';

/** A deployment's events as the Control feed: plain words, the record as evidence, and alarm tags. */

type NameOf = (seatId: string) => string;

/** What the feed needs besides the events: names, the team's title, the agents now, questions and budget. */
export type FeedContext = { nameOf: NameOf; title: string; seats: Record<string, SeatView>; questions: QuestionView[]; budget: TeamBudget | null };

/**
 * The merged list, oldest first: every event (with the progress log's sentence where it has one),
 * an entry for each open question whose event is older than the list, and the run budget's stop or
 * pause. Open questions are tagged `question-waiting` (keyed by question); the alarms module applies
 * the wait threshold. A failure or attention is keyed by its agent and session (see `sessionKeys`).
 */
export function feedFrom(events: EventRecord[], c: FeedContext): FeedItem[] {
  const said = new Map(activityLines(events, c.nameOf).map((l) => [l.seq, l.text]));
  const open = new Set(c.questions.filter((q) => q.state === 'open').map((q) => q.id));
  const sessions = sessionKeys(events, c.seats);
  const items = events.map((e) => eventItem(e, said.get(e.seq) ?? null, c, (b) => alarmOf(b, open, sessions.get(e.seq))));
  const raised = new Set(events.flatMap((e) => (e.body.type === 'question-raised' ? [e.body.question_id] : [])));
  const older = c.questions.filter((q) => open.has(q.id) && !raised.has(q.id)).map((q) => questionItem(q, c.nameOf));
  const run = budgetItem(c.budget);
  return [...items, ...older, ...(run ? [run] : [])].sort((a, b) => a.at - b.at);
}

type Tag = { alarm: string; alarmKey?: string } | null;

function eventItem(e: EventRecord, sentence: string | null, c: FeedContext, tag: (b: EventBody) => Tag): FeedItem {
  const seat = e.subject.kind === 'seat' ? e.subject.id : undefined;
  // Events about the whole team speak of it by its title.
  const who = seat ? c.nameOf(seat) : e.subject.kind === 'rig' ? c.title : 'An agent';
  const text = sentence ?? moreWords(who, e.body) ?? `${who} ${eventWords(e.body.type)}`;
  return { id: `${e.seq}`, at: e.at_ms, ...(seat ? { agent: seat } : {}), kind: e.body.type, text, raw: e, ...tag(e.body) };
}

/** The alarm rule an event raises (see alarms.ts), with the key of what it reports; null for none. */
function alarmOf(b: EventBody, open: Set<string>, session: string | undefined): Tag {
  switch (b.type) {
    case 'seat-lifecycle-changed':
      return lifecycleAlarm(b.lifecycle, session);
    case 'stall-suspected':
      return { alarm: 'stall' };
    case 'context-ceiling-reached':
      return { alarm: 'context-ceiling' };
    case 'limit-reached':
      return { alarm: 'limit' };
    case 'tool-resulted':
      return b.is_error && REFUSAL_WORDS.test(b.output.slice(0, 500)) ? { alarm: 'refused', alarmKey: `call:${b.call_id}` } : null;
    case 'question-raised':
      return open.has(b.question_id) ? { alarm: 'question-waiting', alarmKey: b.question_id } : null;
    default:
      return null;
  }
}

function lifecycleAlarm(lifecycle: SeatView['lifecycle'], session: string | undefined): Tag {
  if (lifecycle !== 'failed' && lifecycle !== 'attention') return null;
  return { alarm: lifecycle, ...(session ? { alarmKey: session } : {}) };
}

/** The key of an agent's session: the same for its failure event and its status (`agentAlarm`). */
export const sessionKey = (seat: string, tenure: number | string) => `${seat}:${tenure}`;

/**
 * For each failure or attention event, its agent's session then: the latest session start before it
 * in the list; else, before the list's first start for that agent, the session before that one; else
 * (no start in the list since) the agent's session now. The list runs up to now, so this holds.
 */
function sessionKeys(events: EventRecord[], seats: Record<string, SeatView>): Map<number, string> {
  const first = firstTenures(events);
  const last = new Map<string, number>();
  const out = new Map<number, string>();
  for (const e of events) {
    if (e.subject.kind !== 'seat') continue;
    const seat = e.subject.id;
    if (e.body.type === 'tenure-started') last.set(seat, e.body.tenure);
    if (e.body.type !== 'seat-lifecycle-changed') continue;
    const tenure = last.get(seat) ?? (first.has(seat) ? first.get(seat)! - 1 : seats[seat]?.tenure);
    out.set(e.seq, sessionKey(seat, tenure ?? 'unknown'));
  }
  return out;
}

function firstTenures(events: EventRecord[]): Map<string, number> {
  const first = new Map<string, number>();
  for (const e of events) if (e.body.type === 'tenure-started' && e.subject.kind === 'seat' && !first.has(e.subject.id)) first.set(e.subject.id, e.body.tenure);
  return first;
}

/** Sentences with the event's own details, for events the progress log has none for. */
function moreWords(who: string, b: EventBody): string | null {
  switch (b.type) {
    case 'question-raised':
      return askedWords(who, b);
    case 'stall-suspected':
      return `${who} may be stuck (suspected): ${b.reason}`;
    case 'context-ceiling-reached':
      return `${who} reached its context limit (${b.used_tokens} of ${b.ceiling} tokens)`;
    case 'limit-reached':
      return `${who} hit a usage limit (resets ${b.resets ?? 'unknown'})`;
    case 'change-proposed':
      return `${who} proposed a change: ${b.summary}`;
    case 'change-decided':
      return `${who}'s change was ${b.approved ? 'approved' : 'rejected'}`;
    case 'rig-desired-changed':
      return `${who} was set to ${b.desired === 'running' ? 'run' : 'stop'}`;
    case 'seat-activity-changed':
      return `${who} is ${ACTIVITY[b.activity].label}`;
    default:
      return null;
  }
}

function askedWords(who: string, b: Pick<QuestionView, 'source' | 'prompt'>): string {
  if (b.source === 'approval') return `${who} asks you to approve a change`;
  if (b.source === 'native-permission') return `${who} asks to use ${b.prompt.tool ?? 'a tool'}`;
  return `${who} asked you a question`;
}

/** An open question asked before the oldest event in the list. */
function questionItem(q: QuestionView, nameOf: NameOf): FeedItem {
  return {
    id: `question:${q.id}`, at: q.opened_at_ms, agent: q.seat, kind: 'question-raised', text: askedWords(nameOf(q.seat), q), raw: q,
    alarm: 'question-waiting', alarmKey: q.id,
  };
}

const BUDGET_WORDS = {
  'wrap-up': 'The run reached its budget; its agents are wrapping up',
  stopped: 'The run reached its budget and stopped',
  paused: 'The run reached its plan limit and is paused until the limit resets',
} as const;

/** The run budget's wrap-up, stop or pause, which the `limit` rule reports. */
function budgetItem(budget: TeamBudget | null): FeedItem | null {
  const s = budget?.run?.state;
  if (!s || s.state === 'normal') return null;
  return { id: `budget:${s.state}:${s.at_ms}`, at: s.at_ms, kind: 'budget', text: BUDGET_WORDS[s.state], raw: budget?.run, alarm: 'limit' };
}

/** Tool calls in each of the last RATE_MINUTES minutes before `now`, oldest first; unknown before `since` (see perMinute). */
export function toolCallsPerMin(events: EventRecord[], now: number, since?: number): (number | null)[] {
  return perMinute(events.filter((e) => e.body.type === 'tool-called').map((e) => e.at_ms), now, since);
}

/**
 * When the earliest current session of an agent that may have a process started (its latest
 * session start in the list). Unknown when nothing runs, or a running agent's start is not in the list.
 */
export function startedAt(events: EventRecord[], seats: SeatView[]): number | undefined {
  const live = seats.filter((s) => hasProcess(s.lifecycle));
  const starts = live.map((s) => events.filter((e) => e.body.type === 'tenure-started' && e.subject.kind === 'seat' && e.subject.id === s.id).at(-1)?.at_ms);
  if (starts.length === 0 || starts.some((t) => t === undefined)) return undefined;
  return Math.min(...(starts as number[]));
}
