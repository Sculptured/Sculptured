import {DismissibleError} from './DismissibleError';
import { TextArea } from './TextArea';
import { errorText } from '../errors';
import { useEffect, useId, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent, type RefObject } from 'react';
import { usePolled } from '../usePolling';
import { api } from '../api';
import type { MessageIntent } from '../generated/MessageIntent';
import type { SeatView } from '../generated/SeatView';
import { LIFECYCLE, isStoppable } from '../status';
import { useRequestOnce } from '../useRequestOnce';
import { WORDS } from '../vocab';
import { Btn } from './ui';

/** Attention is included: its process may still be alive, and if it is not the host says so and the agent is recorded as stopped. */

/** Interrupt, stop and send for one agent. Each action keeps one request id until the server answers. */
/** Why Interrupt is off for an agent that cannot be interrupted, or might not be (unknown). */
function interruptBlocked(seat: SeatView): string {
  return seat.caps ? WORDS.cannotInterrupt : WORDS.interruptUnknown;
}

export function useAgentControls(seat: SeatView, fallbackFocus?: RefObject<HTMLElement | null>) {
  const ids = useId();
  const [interrupting, setInterrupting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const interruptOnce = useRequestOnce();
  const stopOnce = useRequestOnce();
  const wrapOnce = useRequestOnce();
  const [controlMode, setControlMode] = useState<string | null>(null);
  // A local change shows at once; each poll answer replaces it.
  const polled = usePolled(() => api.agentControl(seat.id), 2000, [seat.id]);
  useEffect(() => { if (polled.data) setControlMode(polled.data.mode); }, [polled.data]);
  const wrap = async () => { setError(null); setStopping(true); try { await wrapOnce(id=>api.controlAgent(seat.id,"soft",id)); setControlMode("soft"); } catch(e) {setError(errorText(e));} finally {setStopping(false);} };
  const stoppedByMe = useRef(false);
  const stopBtn = useRef<HTMLButtonElement>(null);

  const canInterrupt = !!seat.caps?.interrupt;
  const inTurn = seat.current_turn !== null;
  const interruptReason = !canInterrupt
    ? interruptBlocked(seat)
    : !inTurn
      ? 'No turn in progress'
      : interrupting
        ? 'Interrupting…'
        : null;
  const stopReason = isStoppable(seat.lifecycle)
    ? null
    : seat.lifecycle === 'stopping'
      ? WORDS.agentAlreadyStopping
      : WORDS.noProcessToStop(LIFECYCLE[seat.lifecycle].label);

  // After a successful stop the status arrives later and disables Stop; if focus was
  // on it, the browser would drop it, so move it to the pane heading.
  useEffect(() => {
    if (stopReason && stoppedByMe.current) {
      stoppedByMe.current = false;
      const a = document.activeElement;
      if (!a || a === document.body || a === stopBtn.current) fallbackFocus?.current?.focus();
    }
  }, [stopReason, fallbackFocus]);

  const interrupt = async () => {
    if (interruptReason) return;
    setError(null);
    setInterrupting(true);
    try {
      await interruptOnce((id) => api.interrupt(seat.id, id));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setInterrupting(false);
    }
  };

  const stop = async (): Promise<boolean> => {
    setError(null);
    setStopping(true);
    try {
      await stopOnce((id) => api.controlAgent(seat.id, "hard", id));
      stoppedByMe.current = true;
      return true;
    } catch (err) {
      setError(errorText(err));
      return false;
    } finally {
      setStopping(false);
    }
  };

  return { wrap, controlMode, ids, seat, canInterrupt, inTurn, interruptReason, interrupt, stopReason, stop, stopping, stopBtn, fallbackFocus, error, setError };
}

/** The agent pane adds Hand over again for Ctrl K: why it cannot run now (null = it can), and asking for it. */
export type AgentControls = ReturnType<typeof useAgentControls> & { handOver?: { reason: string | null; ask: () => void } | null };

/** `[ Interrupt ] i` and `[ Stop agent… ]` with its inline confirm. Disabled controls say why. */
export function AgentActions({ c, name }: { c: AgentControls; name: string }) {
  const ids = c.ids;
  const [confirm, setConfirm] = useState(false);
  const cancelBtn = useRef<HTMLButtonElement>(null);
  const wasConfirming = useRef(false);
  // Focus moves into the inline confirm, and back to the Stop button when it closes.
  useEffect(() => {
    if (confirm) cancelBtn.current?.focus();
    else if (wasConfirming.current) {
      if (c.stopBtn.current?.disabled) c.fallbackFocus?.current?.focus();
      else c.stopBtn.current?.focus();
    }
    wasConfirming.current = confirm;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [confirm]);

  return (
    <div className="agent-actions">
      <Btn onClick={c.wrap} disabled={!!c.stopReason || c.stopping || c.controlMode === "soft"} tip="Finish the current task, summarize and hand off, then stop. About 5K output tokens or 5 minutes; delayed usage can overshoot.">{c.controlMode === "soft" ? "Wrapping up…" : "Soft stop · ~5K"}</Btn>
      {c.controlMode === "budget" && <span role="status">Output budget reached; session stopped.</span>}
      {c.controlMode === "finished" && <span role="status">Wrap-up finished; session stopped.</span>}
      <span className="action">
        <Btn icon="pause" onClick={c.interrupt} disabled={!!c.interruptReason} aria-describedby={c.interruptReason ? `${ids}-int` : undefined} kbd="i" tip="Interrupt the current turn">
          Interrupt
        </Btn>
        {c.interruptReason && (
          <span id={`${ids}-int`} className="reason">
            {c.interruptReason}
          </span>
        )}
      </span>
      {confirm ? (
        <div
          role="group"
          aria-label={WORDS.confirmStopAgent}
          className="confirm"
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.stopPropagation();
              setConfirm(false);
            }
          }}
        >
          <p>{WORDS.stopAgentConsequence(name, c.inTurn)}</p>
          <Btn
            danger
            onClick={async () => {
              if (await c.stop()) setConfirm(false);
            }}
            disabled={c.stopping}
            aria-describedby={c.stopping ? `${ids}-stopping` : undefined}
          >
            {WORDS.confirmStopAgent}
          </Btn>
          <Btn ref={cancelBtn} onClick={() => setConfirm(false)}>
            Cancel
          </Btn>
          {c.stopping && (
            <span id={`${ids}-stopping`} className="reason">
              Stopping…
            </span>
          )}
        </div>
      ) : (
        <span className="action">
          <Btn danger icon="stop" ref={c.stopBtn} onClick={() => setConfirm(true)} disabled={!!c.stopReason} aria-describedby={c.stopReason ? `${ids}-stop` : undefined}>
            Hard stop…
          </Btn>
          {c.stopReason && (
            <span id={`${ids}-stop`} className="reason">
              {c.stopReason}
            </span>
          )}
        </span>
      )}
    </div>
  );
}

const MAX_ROWS = 8;
const CHOOSE_INSTRUCT = 'Choose instruct to interrupt';
const TYPE_FIRST = 'Type a message to send.';

/**
 * One line that grows. Enter sends, Shift+Enter adds a line. Standard clipboard shortcuts stay native.
 * How it is delivered (inform waits for the turn to end, instruct may interrupt it) sits behind `⋯`
 * with "Interrupt current turn"; a small chip by Send says so whenever the choice is not the default.
 */
export function Composer({ c, inputRef }: { c: AgentControls; inputRef?: RefObject<HTMLTextAreaElement | null> }) {
  const ids = useId();
  const seat = c.seat;
  const [text, setText] = useState('');
  const [intent, setIntent] = useState<MessageIntent>('inform');
  const [interrupt, setInterrupt] = useState(false);
  const [sending, setSending] = useState(false);
  const [menu, setMenu] = useState(false);
  const sendOnce = useRequestOnce();
  const ownRef = useRef<HTMLTextAreaElement>(null);
  const box = inputRef ?? ownRef;
  const menuRef = useRef<HTMLDivElement>(null);

  const running = seat.lifecycle === 'running';
  const sendReason = !running
    ? WORDS.sendNeedsRunning(LIFECYCLE[seat.lifecycle].label)
    : sending
      ? 'Sending…'
      : !text.trim()
        ? TYPE_FIRST
        : null;
  const boxReason = !c.canInterrupt
    ? interruptBlocked(c.seat)
    : !c.inTurn
      ? 'No turn in progress'
      : intent !== 'instruct'
        ? CHOOSE_INSTRUCT
        : null;
  const interruptBoxEnabled = intent === 'instruct' && c.canInterrupt && c.inTurn;
  const interrupting = interruptBoxEnabled && interrupt;

  // Grow with the text, up to MAX_ROWS lines.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = 'auto';
    const line = parseFloat(getComputedStyle(el).lineHeight) || 19;
    const max = line * MAX_ROWS + 12;
    if (el.scrollHeight) el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [text, box]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenu(false);
    };
    window.addEventListener('mousedown', close);
    return () => window.removeEventListener('mousedown', close);
  }, [menu]);

  const send = async () => {
    if (!running || !text.trim()) return;
    setSending(true);
    c.setError(null);
    const sent = text;
    try {
      await sendOnce((id) => api.send(seat.id, { text: sent, intent, interrupt: interrupting }, id));
      // Keep anything typed while the request was in flight.
      setText((cur) => (cur === sent ? '' : cur));
      setInterrupt(false);
    } catch (err) {
      c.setError(errorText(err));
    } finally {
      setSending(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void send();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send();
      return;
    }
  };

  return (
    <form className="composer" onSubmit={submit} aria-label="Send a message">
      <div className="composer-line">
        <label htmlFor={`${ids}-msg`} className="sr-only">
          Message
        </label>
        <TextArea
          id={`${ids}-msg`}
          ref={box}
          rows={1}
          value={text}
          spellCheck={false}
          placeholder={running ? 'Message this agent' : ''}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
        />
        {/* Not the default: say so beside Send, in a word. */}
        {intent === 'instruct' && (
          <span className="intent-chip" data-tip={interrupting ? 'Interrupts the current turn' : 'May interrupt its turn'}>
            {interrupting ? 'interrupts now' : 'instruct'}
          </span>
        )}
        <div className="intent-menu" ref={menuRef}>
          <Btn
            variant="ghost"
            icon="more"
            iconOnly
            aria-haspopup="true"
            aria-expanded={menu}
            aria-label={`Message options (intent: ${intent})`}
            tip="How it is delivered"
            onClick={() => setMenu((m) => !m)}
          />
          {menu && (
            <div className="menu" onKeyDown={(e) => e.key === 'Escape' && (e.stopPropagation(), setMenu(false), box.current?.focus())}>
              <div role="radiogroup" aria-label="Intent">
                {(['inform', 'instruct'] as const).map((v) => (
                  <label key={v} className={`menu-item${intent === v ? ' on' : ''}`}>
                    <input
                      type="radio"
                      name={`${ids}-intent`}
                      value={v}
                      checked={intent === v}
                      onChange={() => setIntent(v)}
                    />
                    <span className="radio-mark" aria-hidden="true" />
                    {v}
                    <span className="muted">{v === 'inform' ? ' — delivered when its turn ends' : ' — may interrupt its turn'}</span>
                  </label>
                ))}
              </div>
              <label className={`menu-item check${interruptBoxEnabled ? '' : ' disabled'}`}>
                <input
                  type="checkbox"
                  checked={interrupting}
                  disabled={!interruptBoxEnabled}
                  aria-describedby={boxReason ? `${ids}-box` : undefined}
                  onChange={(e) => setInterrupt(e.target.checked)}
                />
                <span className="check-mark" aria-hidden="true" />
                Interrupt current turn
              </label>
              {boxReason && (
                <span id={`${ids}-box`} className="reason menu-reason">
                  {boxReason}
                </span>
              )}
            </div>
          )}
        </div>
        <Btn type="submit" variant="primary" icon="send" className="send" kbd="⏎" tip="Send · Shift+Enter for a new line" disabled={!!sendReason} aria-describedby={sendReason ? `${ids}-send` : undefined}>
          Send
        </Btn>
      </div>
      {/* Why Send is off: in words when it matters (not running, sending); an empty box only tells screen readers. */}
      {sendReason && (
        <span id={`${ids}-send`} className={sendReason === TYPE_FIRST ? 'sr-only' : 'reason composer-reason'}>
          {sendReason}
        </span>
      )}
      {c.error && (
        <DismissibleError className="tone-red composer-error">
          {c.error}
        </DismissibleError>
      )}
    </form>
  );
}
