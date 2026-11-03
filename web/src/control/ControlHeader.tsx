import { useEffect, useState, type ReactNode } from 'react';
import { compactNumber } from '../format';
import { ActionButton, OutcomeNote, type Outcome } from './ActionButton';
import { span, type ControlCtx } from './context';
import { EmergencyStop } from './EmergencyStop';
import { countsOf, runStatus } from './runStatus';

/**
 * Control's header: a status strip read at a glance (run status, elapsed clock, agents by status,
 * tokens and cost, the master caution), then the whole team's controls. Every reading is a glyph
 * and a word or a number; anything not known says "unknown".
 */

function Tile({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <li className={`ctl-tile${className ? ` ${className}` : ''}`}>
      <span className="ctl-tile-label">{label}</span>
      <span className="ctl-tile-value">{children}</span>
    </li>
  );
}

/** T+hh:mm:ss since the earliest current session started, ticking each second while the team runs. */
function Elapsed({ startedAt, running }: { startedAt?: number; running: boolean }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running || startedAt === undefined) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [running, startedAt]);
  if (!running || startedAt === undefined) return <Tile label="Elapsed" className="is-quiet"><span className="ctl-clock">T+ --:--:--</span> <small>{running ? 'unknown' : 'not running'}</small></Tile>;
  return <Tile label="Elapsed"><span className="ctl-clock" role="timer" aria-label={`Running for ${span(now - startedAt)}`}>T+{span(now - startedAt)}</span></Tile>;
}

function Caution({ ctx }: { ctx: ControlCtx }) {
  const { unacked, list } = ctx.alarms;
  const lit = unacked > 0;
  const words = lit ? `${unacked} not acknowledged` : list.length > 0 ? `${list.length} acknowledged` : 'no alarms';
  return (
    <button type="button" className={`ctl-caution${lit ? ' is-lit' : ''}`} onClick={() => ctx.show('alarms')} aria-label={`Master caution: ${words}. Open Alarms`}>
      <span className="ctl-caution-glyph" aria-hidden="true">{lit ? '▲' : list.length > 0 ? '✓' : '○'}</span>
      <span className="ctl-caution-word">Master caution</span>
      <span className="ctl-caution-count">{lit ? unacked : 0}</span>
      <span className="ctl-caution-sub">{words}</span>
    </button>
  );
}

function Strip({ ctx }: { ctx: ControlCtx }) {
  const { source } = ctx;
  const run = runStatus(source);
  const m = source.metrics;
  const counts = countsOf(source.agents);
  return (
    <ul className="ctl-strip" aria-label="Run status">
      <Tile label="Run" className={`tone-${run.tone}`}><span className="ctl-glyph" aria-hidden="true">{run.glyph}</span> {run.word}</Tile>
      <Elapsed startedAt={source.startedAt} running={source.running} />
      <Tile label="Agents" className="ctl-tile-wide">
        {counts.length === 0 ? 'none yet' : counts.map((c) => <span key={c.word} className={`ctl-count tone-${c.tone}`}><span className="ctl-glyph" aria-hidden="true">{c.glyph}</span> {c.n} {c.word}</span>)}
      </Tile>
      <Tile label="Tokens in / out">{m.tokensIn !== undefined && m.tokensOut !== undefined ? `${compactNumber(m.tokensIn)} / ${compactNumber(m.tokensOut)}` : 'unknown'}</Tile>
      <Tile label="Cost">{m.costUsd !== undefined ? `$${m.costUsd.toFixed(2)}` : 'unknown'}</Tile>
    </ul>
  );
}

function TeamControls({ ctx, onOutcome }: { ctx: ControlCtx; onOutcome(o: Outcome | null): void }) {
  const t = ctx.source.team;
  return (
    <div className="ctl-controls" role="group" aria-label="Team controls">
      <ActionButton action={t.start} label="Start" glyph="▶" className="btn-primary primary" onOutcome={onOutcome} />
      <ActionButton action={t.pauseAll} label="Pause all" glyph="‖" onOutcome={onOutcome} />
      <ActionButton action={t.resumeAll} label="Resume all" glyph="▶" onOutcome={onOutcome} />
      <EmergencyStop action={t.emergencyStop} onOutcome={onOutcome} />
    </div>
  );
}

export function ControlHeader({ ctx }: { ctx: ControlCtx }) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const problems = ctx.source.problems ?? [];
  return (
    <div className="ctl-head">
      <div className="ctl-head-row">
        <Strip ctx={ctx} />
        <div className="ctl-head-right">
          <Caution ctx={ctx} />
          <TeamControls ctx={ctx} onOutcome={setOutcome} />
        </div>
      </div>
      <OutcomeNote o={outcome} dismiss={() => setOutcome(null)} />
      {problems.length > 0 && (
        <ul className="ctl-problems" role="status" aria-label="Not loaded">
          {problems.map((p) => <li key={p}><span aria-hidden="true">◇ </span>{p}</li>)}
        </ul>
      )}
    </div>
  );
}
