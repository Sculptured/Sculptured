import type { EventBody } from '../generated/EventBody';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { SeatView } from '../generated/SeatView';
import type { TeamBudget } from '../generated/TeamBudget';
import { activityLines } from '../progress/sentences';
import { ACTIVITY, hasProcess } from '../status';
import type { FeedItem } from './source';

/** A deployment's events as the Control feed: plain words, the record as evidence, and alarm tags. */

type Body<T extends EventBody['type']> = Extract<EventBody, { type: T }>;
type NameOf = (seatId: string) => string;

/** How many minutes `toolCallsPerMin` covers. */
export const RATE_MINUTES = 15;

/**
 * The merged list, oldest first: every event (with the progress log's sentence where it has one),
 * an entry for each open question whose event is older than the list, and the run budget's stop or
 * pause. Open questions are tagged `question-waiting`; the alarms module applies the wait threshold.
 */
export function feedFrom(events: EventRecord[], nameOf: NameOf, questions: QuestionView[], budget: TeamBudget | null): FeedItem[] {
  const said = new Map(activityLines(events, nameOf).map((l) => [l.seq, l.text]));
  const open = new Set(questions.filter((q) => q.state === 'open').map((q) => q.id));
  const items = events.map((e) => eventItem(e, said.get(e.seq), nameOf, open));
  const raised = new Set(events.flatMap((e) => (e.body.type === 'question-raised' ? [e.body.question_id] : [])));
  const older = questions.filter((q) => open.has(q.id) && !raised.has(q.id)).map((q) => questionItem(q, nameOf));
  const run = budgetItem(budget);
  return [...items, ...older, ...(run ? [run] : [])].sort((a, b) => a.at - b.at);
}

function eventItem(e: EventRecord, sentence: string | undefined, nameOf: NameOf, open: Set<string>): FeedItem {
  const seat = e.subject.kind === 'seat' ? e.subject.id : undefined;
  const who = seat ? nameOf(seat) : 'An agent';
  const text = sentence ?? moreWords(who, e.body) ?? fallbackWords(who, e.body.type);
  const alarm = alarmOf(e.body, open);
  return { id: `${e.seq}`, at: e.at_ms, ...(seat ? { agent: seat } : {}), kind: e.body.type, text, raw: e, ...(alarm ? { alarm } : {}) };
}

/** The alarm rule an event raises (see alarms.ts), if any. */
function alarmOf(b: EventBody, open: Set<string>): string | undefined {
  switch (b.type) {
    case 'seat-lifecycle-changed':
      return b.lifecycle === 'failed' ? 'failed' : b.lifecycle === 'attention' ? 'attention' : undefined;
    case 'stall-suspected':
      return 'stall';
    case 'context-ceiling-reached':
      return 'context-ceiling';
    case 'limit-reached':
      return 'limit';
    case 'question-raised':
      return open.has(b.question_id) ? 'question-waiting' : undefined;
    default:
      return undefined;
  }
}

/** Words for events the progress log leaves out (it has nothing to say about them there). */
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
    default:
      return routineWords(who, b);
  }
}

function askedWords(who: string, b: Body<'question-raised'> | Pick<QuestionView, 'source' | 'prompt'>): string {
  if (b.source === 'approval') return `${who} asks you to approve a change`;
  if (b.source === 'native-permission') return `${who} asks to use ${b.prompt.tool ?? 'a tool'}`;
  return `${who} asked you a question`;
}

function routineWords(who: string, b: EventBody): string | null {
  switch (b.type) {
    case 'question-answered':
      return `${who}'s question was answered`;
    case 'question-expired':
      return `${who}'s question expired`;
    case 'question-withdrawn':
      return `${who} withdrew its question`;
    case 'usage-reported':
      return `${who} reported its token use`;
    case 'turn-started':
      return `${who} started a turn`;
    case 'tenure-started':
      return `${who} started a session`;
    case 'seat-activity-changed':
      return `${who} is ${ACTIVITY[b.activity].label}`;
    default:
      return null;
  }
}

/** Anything else: the record's type in words (the record itself is the evidence). */
function fallbackWords(who: string, type: string): string {
  const words = type.replace(/-/g, ' ').replace(/\bseat\b/g, 'agent').replace(/\brig\b/g, 'team');
  return `${who}: ${words}`;
}

/** An open question asked before the oldest event in the list. */
function questionItem(q: QuestionView, nameOf: NameOf): FeedItem {
  return { id: `question:${q.id}`, at: q.opened_at_ms, agent: q.seat, kind: 'question-raised', text: askedWords(nameOf(q.seat), q), raw: q, alarm: 'question-waiting' };
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

/** Tool calls in each of the last RATE_MINUTES minutes before `now`, oldest first. */
export function toolCallsPerMin(events: EventRecord[], now: number): number[] {
  const out = new Array<number>(RATE_MINUTES).fill(0);
  for (const e of events) {
    if (e.body.type !== 'tool-called') continue;
    const minute = Math.floor(Math.max(0, now - e.at_ms) / 60_000);
    if (minute < RATE_MINUTES) out[RATE_MINUTES - 1 - minute] += 1;
  }
  return out;
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
