import { useState } from 'react';
import { ActionButton, OutcomeNote, useRunAction, type Outcome } from '../ActionButton';
import { nameIn, useControl } from '../context';

/**
 * One agent's controls from the source: Pause (stops at the next safe point and keeps its session),
 * Resume (continues that session), Stop, and (`withMessage`, where no message box is shown already) a
 * message sent as an instruction. Each says why when it cannot run.
 */
export function AgentControlsBar({ agent, withMessage }: { agent: string; withMessage: boolean }) {
  const { source } = useControl();
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [text, setText] = useState('');
  const send = useRunAction((o) => { setOutcome(o); if (o?.tone !== 'bad') setText(''); }, `Sent to ${nameIn(source, agent)}.`);
  const message = source.message(agent, text);
  const why = message.can.ok ? null : message.can.why;
  return (
    <div className="ctl-agent-controls">
      <div className="ctl-row">
        <ActionButton action={source.agentControl(agent, 'pause')} label="Pause" glyph="‖" onOutcome={setOutcome} />
        <ActionButton action={source.agentControl(agent, 'resume')} label="Resume" glyph="▶" onOutcome={setOutcome} />
        <ActionButton action={source.agentControl(agent, 'stop')} label="Stop" glyph="■" className="btn-danger danger" onOutcome={setOutcome} doneText={`Stopping ${nameIn(source, agent)}.`} />
        {withMessage && <form className="ctl-message" onSubmit={(e) => { e.preventDefault(); if (!why) void send.run(message); }}>
          <input aria-label={`Message to ${nameIn(source, agent)}`} placeholder="Message (an instruction; it does not interrupt)" value={text} onChange={(e) => setText(e.target.value)} />
          <button type="submit" className="btn btn-secondary btn-sm" disabled={!!why || send.busy}>Send</button>
          {why && <span className="ctl-why">{why}</span>}
        </form>}
      </div>
      <OutcomeNote o={outcome} dismiss={() => setOutcome(null)} />
    </div>
  );
}
