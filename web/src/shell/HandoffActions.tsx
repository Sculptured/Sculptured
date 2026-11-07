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

type Note = { text: string; error: boolean } | null;

/** Runs one action at a time and keeps its outcome as a note beside the buttons. */
function useRun() {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<Note>(null);
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
  return { busy, note, run };
}

/** Ctrl K's "Hand over again…" behaves as pressing the button: when `ask` changes, and once at once if `askOnMount`. */
function useAsked(ask: number, askOnMount: boolean, onAsked: (() => void) | undefined, trigger: () => void) {
  const lastAsk = useRef(ask);
  useEffect(() => {
    if (ask === lastAsk.current) return;
    lastAsk.current = ask;
    trigger();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ask]);
  useEffect(() => {
    if (!askOnMount) return;
    onAsked?.();
    trigger();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, as it opens
  }, []);
}

type Below = NonNullable<AgentRow['downstream']>;

function HandoffConfirm({ seatId, name, below, close, rerun }: { seatId: string; name: string; below: Below; close: () => void; rerun: () => void }) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => { cancel.current?.focus(); }, []);
  return (
    <div className="handoff-confirm" role="alertdialog" aria-labelledby={`handover-q-${seatId}`} onKeyDown={(e) => { e.stopPropagation(); if (e.key === 'Escape') close(); }}>
      <span id={`handover-q-${seatId}`}>
        Re-run {plural(below.count, 'agent')} downstream of {name}? Each runs once and uses your {below.accounts.join(' and ')} account
        {below.accounts.length > 1 ? 's' : ''}.
      </span>
      <Btn ref={cancel} size="sm" onClick={close}>Cancel</Btn>
      <Btn size="sm" variant="primary" onClick={rerun}>Re-run {plural(below.count, 'agent')}</Btn>
    </div>
  );
}

type ButtonsProps = { a: AgentRow; seatId: string; busy: boolean; run: ReturnType<typeof useRun>['run']; handsTo: string[]; canRestart: boolean; canResend: boolean; canHandOver: boolean; askHandOver: () => void };

function HandoffButtons({ a, seatId, busy, run, handsTo, canRestart, canResend, canHandOver, askHandOver }: ButtonsProps) {
  return (
    <span className="handoff-actions" role="group" aria-label={`Handoffs of ${a.name}`}>
      {canRestart && (
        <Btn size="sm" icon="refresh" disabled={busy} onClick={() => run(() => api.restartSeat(seatId), `Restarting ${a.name}…`)} tip="Relaunch only this agent; it then gets what it is owed">
          Restart this agent
        </Btn>
      )}
      {canResend && (
        <Btn size="sm" icon="send" disabled={busy} onClick={() => run(() => api.resendHandoffs(seatId), 'Handoff sent again.')} tip="Send again what never reached this agent: a handoff, or its Job">
          Resend handoff
        </Btn>
      )}
      {canHandOver && (
        // Agents on your accounts cost money: say how many will run before they do.
        <Btn size="sm" icon="arrow-right" disabled={busy} onClick={askHandOver} tip={`Send ${a.name}'s latest finished turn to ${handsTo.join(', ')}; every agent downstream runs once`}>
          Hand over again
        </Btn>
      )}
    </span>
  );
}

type Part = 'all' | 'urgent' | 'hand-over';

/** What the part of the pane being drawn offers: urgent parts restart and resend, the hand-over part hands over. */
function handoffFlags(a: AgentRow, part: Part) {
  const seat = a.seat;
  const urgent = part !== 'hand-over' && !!seat;
  return {
    stuck: part !== 'hand-over' ? a.stuck : undefined,
    canRestart: urgent && !!a.teamRunning && isRestartable(seat!.lifecycle),
    canResend: urgent && !!a.stuck?.can_resend && seat!.lifecycle === 'running',
    canHandOver: part !== 'urgent' && !!seat && handOverReason(a) === null,
  };
}

const hasContent = (h: ReturnType<typeof useHandoff>) => !!(h.stuck || h.canRestart || h.canResend || h.canHandOver || h.note);

function useHandoff(a: AgentRow, part: Part, ask: number, askOnMount: boolean, onAsked: (() => void) | undefined) {
  const seat = a.seat;
  const { busy, note, run } = useRun();
  const [confirming, setConfirming] = useState(false);
  const handsTo = a.handsTo ?? [];
  const below = a.downstream ?? { count: handsTo.length, accounts: [] };
  const flags = handoffFlags(a, part);
  const handOver = () => seat && run(() => api.handOver(seat.id), `Handing over to ${handsTo.join(', ')}…`);
  const askHandOver = () => (below.accounts.length > 0 ? setConfirming(true) : void handOver());
  useAsked(ask, askOnMount, onAsked, () => { if (flags.canHandOver && !busy) askHandOver(); });
  return { ...flags, busy, note, run, confirming, setConfirming, handsTo, below, handOver, askHandOver };
}

/**
 * The agent's handoff state and what you can do about it. `part="urgent"` (the pane header):
 * "▲ stuck: …" with **Restart this agent** (it needs attention, failed or stopped in a running
 * team) and **Resend handoff** (a handoff never reached it). `part="hand-over"`: **Hand over
 * again** (pass its latest finished turn on to the agents it hands work to, once each), which sits
 * under Details and in Ctrl K unless it is the obvious next step. `ask` changes each time Ctrl K
 * asks for it: the same click, with the same question first for real agents.
 */
export function HandoffActions({ a, part = 'all', ask = 0, askOnMount = false, onAsked }: { a: AgentRow; part?: Part; ask?: number; /** Ask at once (Ctrl K asked before this was on screen). */ askOnMount?: boolean; onAsked?: () => void }) {
  const h = useHandoff(a, part, ask, askOnMount, onAsked);
  const seat = a.seat;
  if (!seat || !hasContent(h)) return null;
  return (
    <div className={`handoff-line handoff-${part}`}>
      {h.stuck && <StuckMark reason={h.stuck.reason} />}
      <HandoffButtons a={a} seatId={seat.id} busy={h.busy} run={h.run} handsTo={h.handsTo} canRestart={h.canRestart} canResend={h.canResend} canHandOver={h.canHandOver} askHandOver={h.askHandOver} />
      {h.confirming && <HandoffConfirm seatId={seat.id} name={a.name} below={h.below} close={() => h.setConfirming(false)} rerun={() => { h.setConfirming(false); void h.handOver(); }} />}
      {h.note && <span role="status" className={h.note.error ? 'tone-red' : 'muted'}>{h.note.text}</span>}
    </div>
  );
}
