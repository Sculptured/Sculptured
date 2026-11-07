import { describe, expect, it } from 'vitest';
import type { EventBody } from '../generated/EventBody';
import type { EventRecord } from '../generated/EventRecord';
import { ev, seatView } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { feedFrom, type FeedContext } from './deploymentFeed';
import { EVENT_WORDS } from './eventWords';

const ctx: FeedContext = { nameOf: (id) => (id === 'dev.a@r' ? 'Builder' : 'b'), title: 'My team', seats: { 'dev.a@r': seatView('a', { tenure: 4 }) }, questions: [], budget: null };
const teamEvent = (seq: number, body: EventBody): EventRecord => ({ ...ev(seq, body), subject: { kind: 'rig', id: 'r' } });

describe('deployment feed words', () => {
  it('speaks of the whole team by its title, in plain words', () => {
    const feed = feedFrom([
      teamEvent(1, { type: 'rig-booted', spec_hash: 'h', seat_count: 2 }),
      teamEvent(2, { type: 'rig-desired-changed', desired: 'stopped' }),
      ev(3, { type: 'handed-over', start_seq: 1, message_id: 'm', from: [], resend_of: null, wave: null }),
      ev(4, { type: 'seat-lifecycle-changed', lifecycle: 'planned', reason: null }),
    ], ctx);
    expect(feed.map((f) => [f.agent, f.text])).toEqual([
      [undefined, 'My team started'],
      [undefined, 'My team was set to stop'],
      ['dev.a@r', 'Builder was given work by another agent'],
      ['dev.a@r', 'Builder changed status'],
    ]);
  });

  it('no event type shows a word of the underlying runtime', () => {
    const root = document.createElement('div');
    root.textContent = Object.values(EVENT_WORDS).join('\n');
    expect(openRigWords(root)).toEqual([]);
  });

  it('a failure is keyed by its session: the start before it, the one before the first start, or now', () => {
    const failed = (seq: number) => ev(seq, { type: 'seat-lifecycle-changed', lifecycle: 'failed', reason: 'x' });
    const started = (seq: number, tenure: number) => ev(seq, { type: 'tenure-started', tenure, cause: 'fresh', driver: 'claude', native_session_id: null, caps: seatView('a').caps! });
    const keys = (events: EventRecord[]) => feedFrom(events, ctx).filter((f) => f.alarm === 'failed').map((f) => f.alarmKey);
    expect(keys([started(1, 2), failed(2)])).toEqual(['dev.a@r:2']);
    expect(keys([failed(1), started(2, 3), failed(3)])).toEqual(['dev.a@r:2', 'dev.a@r:3']);
    expect(keys([failed(1)])).toEqual(['dev.a@r:4']);
  });
});
