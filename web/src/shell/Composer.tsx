import { DismissibleError } from './DismissibleError';
import { TextArea } from './TextArea';
import { errorText } from '../errors';
import { useEffect, useId, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent, type RefObject } from 'react';
import { api } from '../api';
import type { MessageIntent } from '../generated/MessageIntent';
import { LIFECYCLE } from '../status';
import { useRequestOnce } from '../useRequestOnce';
import { WORDS } from '../vocab';
import { interruptBlocked, type AgentControls } from './AgentControls';
import { Btn } from './ui';

const MAX_ROWS = 8;
const CHOOSE_INSTRUCT = 'Choose instruct to interrupt';
const TYPE_FIRST = 'Type a message to send.';

/** Grow with the text, up to MAX_ROWS lines. */
function useGrowingBox(box: RefObject<HTMLTextAreaElement | null>, text: string) {
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = 'auto';
    const line = parseFloat(getComputedStyle(el).lineHeight) || 19;
    const max = line * MAX_ROWS + 12;
    if (el.scrollHeight) el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [text, box]);
}

/** Close an open menu on any press outside it. */
function useCloseOnOutsidePress(open: boolean, ref: RefObject<HTMLElement | null>, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const onPress = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) close(); };
    window.addEventListener('mousedown', onPress);
    return () => window.removeEventListener('mousedown', onPress);
  }, [open, ref, close]);
}

type Delivery = { intent: MessageIntent; interrupting: boolean };

/** Sending keeps anything typed while the request was in flight. */
function useSend(c: AgentControls, running: boolean, text: string, setText: (f: (cur: string) => string) => void, delivery: Delivery, onSent: () => void) {
  const [sending, setSending] = useState(false);
  const sendOnce = useRequestOnce();
  const send = async () => {
    if (!running || !text.trim()) return;
    setSending(true);
    c.setError(null);
    const sent = text;
    try {
      await sendOnce((id) => api.send(c.seat.id, { text: sent, intent: delivery.intent, interrupt: delivery.interrupting }, id));
      setText((cur) => (cur === sent ? '' : cur));
      onSent();
    } catch (err) {
      c.setError(errorText(err));
    } finally {
      setSending(false);
    }
  };
  return { sending, send };
}

type MenuProps = { ids: string; intent: MessageIntent; setIntent: (v: MessageIntent) => void; interrupting: boolean; setInterrupt: (on: boolean) => void; enabled: boolean; reason: string | null };

function IntentOptions({ ids, intent, setIntent, interrupting, setInterrupt, enabled, reason }: MenuProps) {
  return <>
    <div role="radiogroup" aria-label="Intent">
      {(['inform', 'instruct'] as const).map((v) => (
        <label key={v} className={`menu-item${intent === v ? ' on' : ''}`}>
          <input type="radio" name={`${ids}-intent`} value={v} checked={intent === v} onChange={() => setIntent(v)} />
          <span className="radio-mark" aria-hidden="true" />
          {v}
          <span className="muted">{v === 'inform' ? ' — delivered when its turn ends' : ' — may interrupt its turn'}</span>
        </label>
      ))}
    </div>
    <label className={`menu-item check${enabled ? '' : ' disabled'}`}>
      <input type="checkbox" checked={interrupting} disabled={!enabled} aria-describedby={reason ? `${ids}-box` : undefined} onChange={(e) => setInterrupt(e.target.checked)} />
      <span className="check-mark" aria-hidden="true" />
      Interrupt current turn
    </label>
    {reason && <span id={`${ids}-box`} className="reason menu-reason">{reason}</span>}
  </>;
}

function IntentMenu(props: MenuProps & { box: RefObject<HTMLTextAreaElement | null> }) {
  const [menu, setMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  useCloseOnOutsidePress(menu, menuRef, () => setMenu(false));
  return (
    <div className="intent-menu" ref={menuRef}>
      <Btn variant="ghost" icon="more" iconOnly aria-haspopup="true" aria-expanded={menu} aria-label={`Message options (intent: ${props.intent})`} tip="How it is delivered" onClick={() => setMenu((m) => !m)} />
      {menu && (
        <div className="menu" onKeyDown={(e) => e.key === 'Escape' && (e.stopPropagation(), setMenu(false), props.box.current?.focus())}>
          <IntentOptions {...props} />
        </div>
      )}
    </div>
  );
}

function IntentChip({ interrupting }: { interrupting: boolean }) {
  return <span className="intent-chip" data-tip={interrupting ? 'Interrupts the current turn' : 'May interrupt its turn'}>{interrupting ? 'interrupts now' : 'instruct'}</span>;
}

function sendReasonOf(c: AgentControls, running: boolean, sending: boolean, text: string) {
  if (!running) return WORDS.sendNeedsRunning(LIFECYCLE[c.seat.lifecycle].label);
  if (sending) return 'Sending…';
  return text.trim() ? null : TYPE_FIRST;
}

function interruptBoxReason(c: AgentControls, intent: MessageIntent) {
  if (!c.canInterrupt) return interruptBlocked(c.seat);
  if (!c.inTurn) return 'No turn in progress';
  return intent !== 'instruct' ? CHOOSE_INSTRUCT : null;
}

/**
 * One line that grows. Enter sends, Shift+Enter adds a line. Standard clipboard shortcuts stay native.
 * How it is delivered (inform waits for the turn to end, instruct may interrupt it) sits behind `⋯`
 * with "Interrupt current turn"; a small chip by Send says so whenever the choice is not the default.
 */
export function Composer({ c, inputRef }: { c: AgentControls; inputRef?: RefObject<HTMLTextAreaElement | null> }) {
  const ids = useId();
  const [text, setText] = useState('');
  const [intent, setIntent] = useState<MessageIntent>('inform');
  const [interrupt, setInterrupt] = useState(false);
  const ownRef = useRef<HTMLTextAreaElement>(null);
  const box = inputRef ?? ownRef;
  const running = c.seat.lifecycle === 'running';
  const enabled = intent === 'instruct' && c.canInterrupt && c.inTurn;
  const interrupting = enabled && interrupt;
  const { sending, send } = useSend(c, running, text, setText, { intent, interrupting }, () => setInterrupt(false));
  const sendReason = sendReasonOf(c, running, sending, text);
  useGrowingBox(box, text);
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send(); }
  };
  return (
    <form className="composer" onSubmit={(e: FormEvent) => { e.preventDefault(); void send(); }} aria-label="Send a message">
      <div className="composer-line">
        <label htmlFor={`${ids}-msg`} className="sr-only">Message</label>
        <TextArea id={`${ids}-msg`} ref={box} rows={1} value={text} spellCheck={false} placeholder={running ? 'Message this agent' : ''} onChange={(e) => setText(e.target.value)} onKeyDown={onKeyDown} />
        {/* Not the default: say so beside Send, in a word. */}
        {intent === 'instruct' && <IntentChip interrupting={interrupting} />}
        <IntentMenu ids={ids} intent={intent} setIntent={setIntent} interrupting={interrupting} setInterrupt={setInterrupt} enabled={enabled} reason={interruptBoxReason(c, intent)} box={box} />
        <Btn type="submit" variant="primary" icon="send" className="send" kbd="⏎" tip="Send · Shift+Enter for a new line" disabled={!!sendReason} aria-describedby={sendReason ? `${ids}-send` : undefined}>Send</Btn>
      </div>
      {/* Why Send is off: in words when it matters (not running, sending); an empty box only tells screen readers. */}
      {sendReason && <span id={`${ids}-send`} className={sendReason === TYPE_FIRST ? 'sr-only' : 'reason composer-reason'}>{sendReason}</span>}
      {c.error && <DismissibleError className="tone-red composer-error">{c.error}</DismissibleError>}
    </form>
  );
}
