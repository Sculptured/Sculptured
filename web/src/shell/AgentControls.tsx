import { errorText } from '../errors';
import { useEffect, useId, useRef, useState, type RefObject } from 'react';
import { api } from '../api';
import type { ControlMode } from '../generated/ControlMode';
import type { SeatView } from '../generated/SeatView';
import { LIFECYCLE, isStoppable } from '../status';
import { useRequestOnce } from '../useRequestOnce';
import { WORDS } from '../vocab';
import { Btn } from './ui';
import { useControlMode } from './useControlMode';

/** Interrupt and stop for one agent (Composer sends). Each action keeps one request id until the server answers. */
/** Why Interrupt is off for an agent that cannot be interrupted, or might not be (unknown). */
export function interruptBlocked(seat: SeatView): string {
  return seat.caps ? WORDS.cannotInterrupt : WORDS.interruptUnknown;
}

/** `knownMode`: the agent's control mode as the caller reads it (see useControlMode). */
export function useAgentControls(seat: SeatView, fallbackFocus?: RefObject<HTMLElement | null>, knownMode?: ControlMode | null) {
  const ids = useId();
  const [error, setError] = useState<string | null>(null);
  const [controlMode, setControlMode] = useControlMode(seat.id, knownMode);
  const [stopping, setStopping] = useState(false);
  const stoppedByMe = useRef(false);
  const stopBtn = useRef<HTMLButtonElement>(null);
  const canInterrupt = !!seat.caps?.interrupt;
  const inTurn = seat.current_turn !== null;
  const { interrupt, interruptReason } = useInterrupt(seat, setError, canInterrupt, inTurn);
  const stopReason = stopReasonOf(seat);
  useRefocusAfterStop(stopReason, stoppedByMe, stopBtn, fallbackFocus);
  const wrap = useStopRequest(seat, 'soft', setError, setStopping, () => setControlMode('soft'));
  const stop = useStopRequest(seat, 'hard', setError, setStopping, () => { stoppedByMe.current = true; });
  return { wrap, controlMode, ids, seat, canInterrupt, inTurn, interruptReason, interrupt, stopReason, stop, stopping, stopBtn, fallbackFocus, error, setError };
}

function stopReasonOf(seat: SeatView): string | null {
  if (isStoppable(seat.lifecycle)) return null;
  return seat.lifecycle === 'stopping' ? WORDS.agentAlreadyStopping : WORDS.noProcessToStop(LIFECYCLE[seat.lifecycle].label);
}

function useInterrupt(seat: SeatView, setError: (message: string | null) => void, canInterrupt: boolean, inTurn: boolean) {
  const [interrupting, setInterrupting] = useState(false);
  const interruptOnce = useRequestOnce();
  const interruptReason = !canInterrupt ? interruptBlocked(seat) : !inTurn ? 'No turn in progress' : interrupting ? 'Interrupting…' : null;
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
  return { interrupt, interruptReason };
}

/** A soft (wrap up) or hard stop: one request id until answered; true when the server accepted it. */
function useStopRequest(seat: SeatView, mode: 'soft' | 'hard', setError: (message: string | null) => void, setStopping: (on: boolean) => void, onDone: () => void) {
  const once = useRequestOnce();
  return async (): Promise<boolean> => {
    setError(null);
    setStopping(true);
    try {
      await once((id) => api.controlAgent(seat.id, mode, id));
      onDone();
      return true;
    } catch (err) {
      setError(errorText(err));
      return false;
    } finally {
      setStopping(false);
    }
  };
}

/** After a successful stop the status arrives later and disables Stop; if focus was
 * on it, the browser would drop it, so move it to the pane heading. */
function useRefocusAfterStop(stopReason: string | null, stoppedByMe: RefObject<boolean>, stopBtn: RefObject<HTMLButtonElement | null>, fallbackFocus?: RefObject<HTMLElement | null>) {
  useEffect(() => {
    if (stopReason && stoppedByMe.current) {
      stoppedByMe.current = false;
      const a = document.activeElement;
      if (!a || a === document.body || a === stopBtn.current) fallbackFocus?.current?.focus();
    }
  }, [stopReason, fallbackFocus, stoppedByMe, stopBtn]);
}

/** The agent pane adds Hand over again for Ctrl K: why it cannot run now (null = it can), and asking for it. */
export type AgentControls = ReturnType<typeof useAgentControls> & { handOver?: { reason: string | null; ask: () => void } | null };

function StopConfirm({ c, name, close }: { c: AgentControls; name: string; close: () => void }) {
  const ids = c.ids;
  const cancelBtn = useRef<HTMLButtonElement>(null);
  useEffect(() => { cancelBtn.current?.focus(); }, []);
  return (
    <div role="group" aria-label={WORDS.confirmStopAgent} className="confirm" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } }}>
      <p>{WORDS.stopAgentConsequence(name, c.inTurn)}</p>
      <Btn danger onClick={async () => { if (await c.stop()) close(); }} disabled={c.stopping} aria-describedby={c.stopping ? `${ids}-stopping` : undefined}>{WORDS.confirmStopAgent}</Btn>
      <Btn ref={cancelBtn} onClick={close}>Cancel</Btn>
      {c.stopping && <span id={`${ids}-stopping`} className="reason">Stopping…</span>}
    </div>
  );
}

function HardStopButton({ c, ask }: { c: AgentControls; ask: () => void }) {
  const ids = c.ids;
  return (
    <span className="action">
      <Btn danger icon="stop" ref={c.stopBtn} onClick={ask} disabled={!!c.stopReason} aria-describedby={c.stopReason ? `${ids}-stop` : undefined}>Hard stop…</Btn>
      {c.stopReason && <span id={`${ids}-stop`} className="reason">{c.stopReason}</span>}
    </span>
  );
}

function InterruptButton({ c }: { c: AgentControls }) {
  const ids = c.ids;
  return (
    <span className="action">
      <Btn icon="pause" onClick={c.interrupt} disabled={!!c.interruptReason} aria-describedby={c.interruptReason ? `${ids}-int` : undefined} kbd="i" tip="Interrupt the current turn">Interrupt</Btn>
      {c.interruptReason && <span id={`${ids}-int`} className="reason">{c.interruptReason}</span>}
    </span>
  );
}

/** Focus moves into the inline confirm, and back to the Stop button when it closes. */
function useConfirmFocus(c: AgentControls, confirm: boolean) {
  const wasConfirming = useRef(false);
  useEffect(() => {
    if (!confirm && wasConfirming.current) {
      if (c.stopBtn.current?.disabled) c.fallbackFocus?.current?.focus();
      else c.stopBtn.current?.focus();
    }
    wasConfirming.current = confirm;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [confirm]);
}

/** `[ Interrupt ] i` and `[ Stop agent… ]` with its inline confirm. Disabled controls say why. */
export function AgentActions({ c, name }: { c: AgentControls; name: string }) {
  const [confirm, setConfirm] = useState(false);
  useConfirmFocus(c, confirm);
  return (
    <div className="agent-actions">
      <Btn onClick={c.wrap} disabled={!!c.stopReason || c.stopping || c.controlMode === 'soft'} tip="Finish the current task, summarize and hand off, then stop. About 5K output tokens or 5 minutes; delayed usage can overshoot.">{c.controlMode === 'soft' ? 'Wrapping up…' : 'Soft stop · ~5K'}</Btn>
      {c.controlMode === 'budget' && <span role="status">Output budget reached; session stopped.</span>}
      {c.controlMode === 'finished' && <span role="status">Wrap-up finished; session stopped.</span>}
      <InterruptButton c={c} />
      {confirm ? <StopConfirm c={c} name={name} close={() => setConfirm(false)} /> : <HardStopButton c={c} ask={() => setConfirm(true)} />}
    </div>
  );
}
