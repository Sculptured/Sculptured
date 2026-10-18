import { useEffect, useId, useRef, useState, type RefObject } from 'react';
import { api, ApiError, newRequestId } from '../api';
import { errorText } from '../errors';
import { plural } from '../format';
import type { MessageIntent } from '../generated/MessageIntent';
import { LIFECYCLE } from '../status';
import type { AgentRow } from './model';
import { Btn, Icon, StateMark, VendorMark } from './ui';

/** Messages can only go to a running agent (as in an agent's own message box). */
export const canReceive = (a: AgentRow) => a.seat?.lifecycle === 'running';

/** Why an agent cannot be sent a message now; null when it can. */
const whyNot = (a: AgentRow) => (canReceive(a) ? null : a.seat ? `${LIFECYCLE[a.seat.lifecycle].label}; only running agents get messages` : 'status unknown');

const names = (xs: AgentRow[]) => (xs.length <= 1 ? xs.map((a) => a.name).join('') : `${xs.slice(0, -1).map((a) => a.name).join(', ')} and ${xs.at(-1)!.name}`);

type Result = { ok: true } | { ok: false; error: string };
type Phase = 'compose' | 'confirm' | 'sending' | 'done';
type Props = {
  /** Every agent of the team. */
  agents: AgentRow[];
  /** Ticked at the start; empty = every agent that can receive a message. */
  initial: string[];
  team: string;
  onClose: () => void;
};

/**
 * One message to several agents of a running team. Sending to more than one asks first, naming
 * every recipient. Each agent gets its own request, and the result says which got it and which
 * did not (in plain words), so the failed ones can be sent again.
 */
export function BroadcastDialog({ agents, initial, team, onClose }: Props) {
  const ids = useId();
  const r = useRecipients(agents, initial);
  const [text, setText] = useState('');
  const [intent, setIntent] = useState<MessageIntent>('inform');
  const s = useSender();
  const refs = useStepFocus(s.phase);
  const sendReason = r.eligible.length === 0 ? 'No agent of this team is running.' : r.chosen.length === 0 ? 'Choose at least one agent.' : !text.trim() ? 'Type a message to send.' : null;
  const send = (targets: AgentRow[]) => void s.sendTo(targets, { text, intent, interrupt: false });
  const submit = () => {
    if (sendReason) return;
    if (r.chosen.length > 1) s.setPhase('confirm');
    else send(r.chosen);
  };
  const compose = { ids, r, text, setText, intent, setIntent, sendReason, submit, box: refs.box, onClose };
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && s.phase !== 'sending' && onClose()}>
      <div className="panel broadcast" role={s.phase === 'confirm' ? 'alertdialog' : 'dialog'} aria-modal="true" aria-labelledby={`${ids}-title`} aria-describedby={s.phase === 'confirm' ? `${ids}-confirm` : undefined}
        onKeyDown={dialogKeys(s.phase, s.setPhase, onClose)}>
        <TitleBar id={`${ids}-title`} title={s.phase === 'confirm' ? `Send to ${plural(r.chosen.length, 'agent')}?` : `Message agents · ${team}`} busy={s.phase === 'sending'} onClose={onClose} />
        {s.phase === 'compose' && <ComposeStep {...compose} />}
        {s.phase === 'confirm' && <ConfirmStep id={`${ids}-confirm`} recipients={r.chosen} text={text} back={refs.back} onBack={() => s.setPhase('compose')} onSend={() => send(r.chosen)} />}
        {(s.phase === 'sending' || s.phase === 'done') && <ResultStep recipients={r.chosen} results={s.results} done={s.phase === 'done'} close={refs.close} onRetry={send} onClose={onClose} />}
      </div>
    </div>
  );
}

function TitleBar({ id, title, busy, onClose }: { id: string; title: string; busy: boolean; onClose: () => void }) {
  return (
    <div className="panel-bar">
      <span className="panel-title" id={id}>
        <Icon name="message" size={15} /> {title}
      </span>
      <span className="panel-rule" aria-hidden="true" />
      <Btn variant="ghost" icon="x" iconOnly onClick={onClose} aria-label="Close" kbd="esc" disabled={busy} />
    </div>
  );
}

/** Esc goes back a step (or closes); no other key reaches the board or the app behind. */
function dialogKeys(phase: Phase, setPhase: (p: Phase) => void, onClose: () => void) {
  return (e: React.KeyboardEvent) => {
    if (e.key !== 'Tab') e.stopPropagation();
    if (e.key !== 'Escape') return;
    e.preventDefault();
    if (phase === 'confirm') setPhase('compose');
    else if (phase !== 'sending') onClose();
  };
}

type Recipients = ReturnType<typeof useRecipients>;

function useRecipients(agents: AgentRow[], initial: string[]) {
  const eligible = agents.filter(canReceive);
  const [to, setTo] = useState<Set<string>>(() => new Set((initial.length ? initial : eligible.map((a) => a.id)).filter((id) => eligible.some((a) => a.id === id))));
  const toggle = (id: string) =>
    setTo((cur) => {
      const next = new Set(cur);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  return { agents, eligible, to, setTo, toggle, chosen: agents.filter((a) => to.has(a.id)) };
}

/** Sends to each agent on its own. A request id is kept across a network failure, so sending again delivers once. */
function useSender() {
  const [phase, setPhase] = useState<Phase>('compose');
  const [results, setResults] = useState<Record<string, Result>>({});
  const requestIds = useRef(new Map<string, string>());
  const sendOne = async (a: AgentRow, body: { text: string; intent: MessageIntent; interrupt: boolean }): Promise<[string, Result]> => {
    const id = requestIds.current.get(a.id) ?? newRequestId();
    requestIds.current.set(a.id, id);
    try {
      await api.send(a.seat!.id, body, id);
      requestIds.current.delete(a.id);
      return [a.id, { ok: true }];
    } catch (e) {
      if (e instanceof ApiError) requestIds.current.delete(a.id);
      return [a.id, { ok: false, error: errorText(e) }];
    }
  };
  const sendTo = async (targets: AgentRow[], body: { text: string; intent: MessageIntent; interrupt: boolean }) => {
    setPhase('sending');
    const out = await Promise.all(targets.map((a) => sendOne(a, body)));
    setResults((cur) => ({ ...cur, ...Object.fromEntries(out) }));
    setPhase('done');
  };
  return { phase, setPhase, results, sendTo };
}

/** Focus: the message box first, Back when asked to confirm, Close once sent. Then back where it was. */
function useStepFocus(phase: Phase) {
  const box = useRef<HTMLTextAreaElement>(null);
  const back = useRef<HTMLButtonElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const restore = document.activeElement as HTMLElement | null;
    return () => {
      if (restore && document.contains(restore)) restore.focus?.();
    };
  }, []);
  useEffect(() => {
    if (phase === 'confirm') back.current?.focus();
    else if (phase === 'done') close.current?.focus();
    else if (phase === 'compose') box.current?.focus();
  }, [phase]);
  return { box, back, close };
}

type ComposeProps = {
  ids: string;
  r: Recipients;
  text: string;
  setText: (t: string) => void;
  intent: MessageIntent;
  setIntent: (i: MessageIntent) => void;
  sendReason: string | null;
  submit: () => void;
  box: RefObject<HTMLTextAreaElement | null>;
  onClose: () => void;
};

function ComposeStep({ ids, r, text, setText, intent, setIntent, sendReason, submit, box, onClose }: ComposeProps) {
  return (
    <form className="broadcast-body" onSubmit={(e) => { e.preventDefault(); submit(); }}>
      <RecipientList ids={ids} r={r} />
      <label className="broadcast-msg">
        <span>Message</span>
        <textarea ref={box} className="fld-input" rows={4} value={text} spellCheck={false} placeholder="the same message goes to each agent you chose"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); submit(); } }} />
      </label>
      <div className="broadcast-intent" role="radiogroup" aria-label="How it arrives">
        {(['inform', 'instruct'] as const).map((v) => (
          <label key={v}>
            <input type="radio" name={`${ids}-intent`} checked={intent === v} onChange={() => setIntent(v)} />
            {v}
            <span className="muted">{v === 'inform' ? ' — waits for the current turn' : ' — may interrupt'}</span>
          </label>
        ))}
      </div>
      <div className="broadcast-actions">
        {sendReason && <span id={`${ids}-send-why`} className="reason">{sendReason}</span>}
        <span className="pane-spacer" />
        <Btn onClick={onClose}>Cancel</Btn>
        <Btn type="submit" variant="primary" icon="send" disabled={!!sendReason} aria-describedby={sendReason ? `${ids}-send-why` : undefined} kbd="Ctrl+⏎">
          {r.chosen.length > 1 ? `Send to ${r.chosen.length} agents…` : 'Send'}
        </Btn>
      </div>
    </form>
  );
}

function RecipientList({ ids, r }: { ids: string; r: Recipients }) {
  return (
    <fieldset className="broadcast-to">
      <legend>
        To <span className="muted">· {r.chosen.length} of {r.eligible.length} running</span>
      </legend>
      <div className="broadcast-quick">
        <button type="button" className="link" onClick={() => r.setTo(new Set(r.eligible.map((a) => a.id)))}>All running</button>
        <button type="button" className="link" onClick={() => r.setTo(new Set())}>None</button>
      </div>
      <ul>
        {r.agents.map((a) => {
          const why = whyNot(a);
          return (
            <li key={a.id} className={why ? 'off' : undefined}>
              <label>
                <input type="checkbox" checked={r.to.has(a.id)} disabled={!!why} onChange={() => r.toggle(a.id)} aria-describedby={why ? `${ids}-why-${a.id}` : undefined} />
                <StateMark state={a.state} />
                <VendorMark v={a.vendor} size="xs" />
                <span className="broadcast-name">{a.name}</span>
                {why && <span className="reason" id={`${ids}-why-${a.id}`}>{why}</span>}
              </label>
            </li>
          );
        })}
      </ul>
    </fieldset>
  );
}

type ConfirmProps = { id: string; recipients: AgentRow[]; text: string; back: RefObject<HTMLButtonElement | null>; onBack: () => void; onSend: () => void };

function ConfirmStep({ id, recipients, text, back, onBack, onSend }: ConfirmProps) {
  return (
    <div className="broadcast-body">
      <p id={id}>The same message goes to {names(recipients)}. Each agent gets its own copy.</p>
      <blockquote className="broadcast-preview">{text}</blockquote>
      <div className="broadcast-actions">
        <span className="pane-spacer" />
        <Btn ref={back} onClick={onBack}>Back</Btn>
        <Btn variant="primary" icon="send" onClick={onSend}>Send to {recipients.length} agents</Btn>
      </div>
    </div>
  );
}

type ResultProps = { recipients: AgentRow[]; results: Record<string, Result>; done: boolean; close: RefObject<HTMLButtonElement | null>; onRetry: (xs: AgentRow[]) => void; onClose: () => void };

function ResultStep({ recipients, results, done, close, onRetry, onClose }: ResultProps) {
  const failed = recipients.filter((a) => results[a.id] && !results[a.id].ok);
  const sent = recipients.filter((a) => results[a.id]?.ok);
  const summary = !done
    ? `Sending to ${plural(recipients.length, 'agent')}…`
    : failed.length === 0
      ? `Sent to ${plural(sent.length, 'agent')}.`
      : `Sent to ${sent.length} of ${recipients.length}. ${plural(failed.length, 'agent')} did not get it.`;
  return (
    <div className="broadcast-body">
      <p role="status">{summary}</p>
      <ul className="broadcast-results" aria-label="Who got the message">
        {recipients.map((a) => <ResultLine key={a.id} a={a} r={done || results[a.id]?.ok ? results[a.id] : undefined} />)}
      </ul>
      {done && (
        <div className="broadcast-actions">
          <span className="pane-spacer" />
          {failed.length > 0 && <Btn icon="refresh" onClick={() => onRetry(failed)}>Send again to {plural(failed.length, 'agent')}</Btn>}
          <Btn ref={close} variant={failed.length ? undefined : 'primary'} onClick={onClose}>Close</Btn>
        </div>
      )}
    </div>
  );
}

function ResultLine({ a, r }: { a: AgentRow; r?: Result }) {
  return (
    <li className={!r ? 'muted' : r.ok ? 'tone-green' : 'tone-red'}>
      <span aria-hidden="true">{!r ? '◌' : r.ok ? '✓' : '▲'} </span>
      <span className="broadcast-name">{a.name}</span>
      {' — '}
      {!r ? 'sending…' : r.ok ? 'sent' : `not sent: ${r.error}`}
    </li>
  );
}

/** Run view: while agents are picked (Shift+click), what can be done with them. */
export function RunSelectionBar({ picked, onMessage, onClear }: { picked: AgentRow[]; onMessage: () => void; onClear: () => void }) {
  const ids = useId();
  const none = picked.filter(canReceive).length === 0;
  return (
    <div className="selection-bar run-selection" role="toolbar" aria-label={`${plural(picked.length, 'agent')} selected`}>
      <span className="sel-count" role="status">
        <strong>{picked.length}</strong> {picked.length === 1 ? 'agent' : 'agents'} selected
      </span>
      <span className="sel-sep" aria-hidden="true" />
      <Btn size="sm" variant="primary" icon="message" onClick={onMessage} disabled={none} aria-describedby={none ? `${ids}-why` : undefined}>
        Message…
      </Btn>
      {none && <span id={`${ids}-why`} className="reason sel-reason">None of them is running.</span>}
      <Btn size="sm" variant="ghost" icon="x" onClick={onClear} aria-label="Clear the selection" kbd="esc">
        <kbd>Esc</kbd>
      </Btn>
    </div>
  );
}
