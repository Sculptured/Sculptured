import { useEffect, useRef, useState } from 'react';
import { isRestartable } from '../status';
import { api } from '../api';
import { errorText } from '../errors';
import type { AgentRow } from './model';
import { Btn, StuckMark } from './ui';
import { plural } from '../format';


/** Whether "Hand over again" can run for this agent now, and why not when it cannot. */
export function handOverReason(a: AgentRow): string | null {
  if (!a.teamRunning) return 'The team is not running.';
  if ((a.handsTo ?? []).length === 0) return 'This agent hands work to no one.';
  if (a.seat?.lifecycle !== 'running') return 'Only a running agent can hand over.';
  return null;
}

/**
 * The agent's handoff state and what you can do about it. `part="urgent"` (the pane header):
 * "▲ stuck: …" with **Restart this agent** (it needs attention, failed or stopped in a running
 * team) and **Resend handoff** (a handoff never reached it). `part="hand-over"`: **Hand over
 * again** (pass its latest finished turn on to the agents it hands work to, once each), which sits
 * under Details and in Ctrl K unless it is the obvious next step. `ask` changes each time Ctrl K
 * asks for it: the same click, with the same question first for real agents.
 */
export function HandoffActions({ a, part = 'all', ask = 0, askOnMount = false, onAsked }: { a: AgentRow; part?: 'all' | 'urgent' | 'hand-over'; ask?: number; /** Ask at once (Ctrl K asked before this was on screen). */ askOnMount?: boolean; onAsked?: () => void }) {
  const seat = a.seat;
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirming) cancel.current?.focus();
  }, [confirming]);
  const handsTo = a.handsTo ?? [];
  const below = a.downstream ?? { count: handsTo.length, accounts: [] };
  const urgent = part !== 'hand-over';
  const canHandOver = part !== 'urgent' && !!seat && handOverReason(a) === null;
  const run = async (action: () => Promise<unknown>, done: string) => {
    setBusy(true);
    setNote(null);
    try {
      await action();
      setNote({ text: done, error: false });
    } catch (e) {
      setNote({ text: errorText(e), error: true });
    } finally {
      setBusy(false);
    }
  };
  const handOver = () => seat && run(() => api.handOver(seat.id), `Handing over to ${handsTo.join(', ')}…`);
  const askHandOver = () => (below.accounts.length > 0 ? setConfirming(true) : void handOver());
  // Ctrl K's "Hand over again…": the same as pressing the button.
  const lastAsk = useRef(ask);
  useEffect(() => {
    if (ask === lastAsk.current) return;
    lastAsk.current = ask;
    if (canHandOver && !busy) askHandOver();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ask]);
  useEffect(() => {
    if (!askOnMount) return;
    onAsked?.();
    if (canHandOver && !busy) askHandOver();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, as it opens
  }, []);
  if (!seat) return null;
  const stuck = urgent ? a.stuck : undefined;
  const canRestart = urgent && !!a.teamRunning && isRestartable(seat.lifecycle);
  const canResend = urgent && !!a.stuck?.can_resend && seat.lifecycle === 'running';
  if (!stuck && !canRestart && !canResend && !canHandOver && !note) return null;


  return (
    <div className={`handoff-line handoff-${part}`}>
      {stuck && <StuckMark reason={stuck.reason} />}
      <span className="handoff-actions" role="group" aria-label={`Handoffs of ${a.name}`}>
        {canRestart && (
          <Btn size="sm" icon="refresh" disabled={busy} onClick={() => run(() => api.restartSeat(seat.id), `Restarting ${a.name}…`)} tip="Relaunch only this agent; it then gets what it is owed">
            Restart this agent
          </Btn>
        )}
        {canResend && (
          <Btn size="sm" icon="send" disabled={busy} onClick={() => run(() => api.resendHandoffs(seat.id), 'Handoff sent again.')} tip="Send again what never reached this agent: a handoff, or its Job">
            Resend handoff
          </Btn>
        )}
        {canHandOver && (
          <Btn
            size="sm"
            icon="arrow-right"
            disabled={busy}
            // Real agents cost money: say how many will run before they do. Practice agents are free.
            onClick={askHandOver}
            tip={`Send ${a.name}'s latest finished turn to ${handsTo.join(', ')}; every agent downstream runs once`}
          >
            Hand over again
          </Btn>
        )}
      </span>
      {confirming && (
        <div
          className="handoff-confirm"
          role="alertdialog"
          aria-labelledby={`handover-q-${seat.id}`}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Escape') setConfirming(false);
          }}
        >
          <span id={`handover-q-${seat.id}`}>
            Re-run {plural(below.count, 'agent')} downstream of {a.name}? Each runs once and uses your {below.accounts.join(' and ')} account
            {below.accounts.length > 1 ? 's' : ''}.
          </span>
          <Btn ref={cancel} size="sm" onClick={() => setConfirming(false)}>
            Cancel
          </Btn>
          <Btn
            size="sm"
            variant="primary"
            onClick={() => {
              setConfirming(false);
              void handOver();
            }}
          >
            Re-run {plural(below.count, 'agent')}
          </Btn>
        </div>
      )}
      {note && (
        <span role="status" className={note.error ? 'tone-red' : 'muted'}>
          {note.text}
        </span>
      )}
    </div>
  );
}
