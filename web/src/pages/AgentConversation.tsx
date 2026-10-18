import { useCallback, useEffect } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import type { EventRecord } from '../generated/EventRecord';
import { agentPagePath } from '../paths';
import { Composer, type AgentControls } from '../shell/AgentControls';
import { ChatTextMode } from '../shell/ChatTextMode';
import { HandoffActions } from '../shell/HandoffActions';
import type { AgentRow } from '../shell/model';
import { OpenQuestions } from '../shell/OpenQuestions';
import { useShell } from '../shell/ShellContext';
import { SignInNeeded } from '../shell/SignInNeeded';
import { StallNotice } from '../shell/StallNotice';
import { Transcript } from '../shell/Transcript';
import { Icon, WaitingMark } from '../shell/ui';
import { useSeatEvents } from '../useSeatEvents';
import { WORDS } from '../vocab';

/**
 * Above the message box of a first agent with no Job (`AgentRow.firstMessageIsWork`): the next
 * message is its work. Hidden once it has read a message in its current session (a start, or a
 * restart, begins a new one): that message was its work, and the next is not.
 */
function FirstMessageHint({ seatId, feed, epoch }: { seatId: string; feed: EventRecord[]; epoch: number }) {
  const { events } = useSeatEvents(seatId, feed, epoch);
  let readThisSession = false;
  for (const e of events) {
    if (e.body.type === 'tenure-started') readThisSession = false;
    else if (e.body.type === 'message-consumed') readThisSession = true;
  }
  if (readThisSession) return null;
  return <p className="composer-hint" role="note"><Icon name="message" size={13} className="muted" /> {WORDS.firstMessageIsWork}</p>;
}

/** What needs you about this agent, above its transcript: questions, sign-in, a stall, a wait, its reason. */
function Notices({ a, feed, handOverInline, askHandOver }: { a: AgentRow; feed: EventRecord[]; handOverInline: boolean; askHandOver: number }) {
  const shell = useShell();
  const seat = a.seat!;
  const reason = seat.lifecycle_reason;
  return (
    <div className="ap-notices">
      <HandoffActions key={seat.id} a={a} part={handOverInline ? 'all' : 'urgent'} ask={askHandOver} />
      <SignInNeeded a={a} feed={feed} />
      <OpenQuestions key={`q-${seat.id}`} seat={seat.id} name={a.name} feed={feed} focusToken={shell.answerToken} />
      <StallNotice seat={seat} />
      {a.waitingFor && a.waitingFor.length > 0 && !a.stuck && (
        <p className="pane-waiting">
          <WaitingMark names={a.waitingFor} />
          <span className="muted"> — {a.waitingFor.length > 1 ? 'starts when they have all finished; their results are passed along' : `starts when ${a.waitingFor[0]} finishes; ${a.waitingFor[0]}'s result is passed along`}</span>
        </p>
      )}
      {reason && <p className={`reason full-reason${['attention', 'failed'].includes(seat.lifecycle) ? ' tone-red' : ''}`}>Reason: <span data-raw-evidence>{reason}</span></p>}
    </div>
  );
}

type Props = { a: AgentRow; c: AgentControls; feed: EventRecord[]; epoch: number; searchSlot: HTMLElement | null; handOverInline: boolean; askHandOver: number };

/** The Conversation tab: the transcript and the message box, full width at a comfortable measure. */
export function AgentConversation({ a, c, feed, epoch, searchSlot, handOverInline, askHandOver }: Props) {
  const shell = useShell();
  const seat = a.seat!;
  // Activity opens a point in the transcript (`?item=`); back returns to the picture.
  const location = useLocation();
  const navigate = useNavigate();
  const focusKey = new URLSearchParams(location.search).get('item');
  const fromVisual = (location.state as { from?: string } | null)?.from === 'visual';
  const backToVisual = useCallback(() => navigate(-1), [navigate]);
  // Answer lands in the message box (then in the agent's open question once it has loaded).
  useEffect(() => {
    if (!shell.answerNext.current) return;
    shell.answerNext.current = false;
    shell.refs.composer.current?.focus();
  });
  return (
    <ChatTextMode className={`agent-pane ap-chat ${a.vendor.cls} st-${a.state.key}`} name={a.name} onOpen={() => {}}>
      <Notices a={a} feed={feed} handOverInline={handOverInline} askHandOver={askHandOver} />
      <div className="pane-body">
        <Transcript key={seat.id} seatId={seat.id} feed={feed} epoch={epoch} vendor={a.vendor} name={a.name} focusKey={focusKey}
          onReturn={fromVisual ? backToVisual : undefined} returnLabel="Activity" toolsSlot={searchSlot} />
      </div>
      {a.firstMessageIsWork && !seat.current_turn && <FirstMessageHint seatId={seat.id} feed={feed} epoch={epoch} />}
      <Composer key={seat.id} c={c} inputRef={shell.refs.composer} />
    </ChatTextMode>
  );
}

/** Where Activity's "show in transcript" goes. */
export const transcriptItemPath = (team: string, seat: string, key: string) => `${agentPagePath(team, seat, 'conversation')}?item=${encodeURIComponent(key)}`;
