import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { compactNumber } from '../format';
import { useMobile } from '../workspace/MobileWorkspace';
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
function Clock({ startedAt, running }: { startedAt?: number; running: boolean }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running || startedAt === undefined) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [running, startedAt]);
  if (!running || startedAt === undefined) return <><span className="ctl-clock is-quiet">T+ --:--:--</span> <small>{running ? 'unknown' : 'not running'}</small></>;
  return <span className="ctl-clock" role="timer" aria-label={`Running for ${span(now - startedAt)}`}>T+{span(now - startedAt)}</span>;
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
      <Tile label="Elapsed"><Clock startedAt={source.startedAt} running={source.running} /></Tile>
      <Tile label="Agents" className="ctl-tile-wide">
        {counts.length === 0 ? 'none yet' : counts.map((c) => <span key={c.word} className={`ctl-count tone-${c.tone}`}><span className="ctl-glyph" aria-hidden="true">{c.glyph}</span> {c.n} {c.word}</span>)}
      </Tile>
      <Tile label="Tokens in / out">{m.tokensIn !== undefined && m.tokensOut !== undefined ? `${compactNumber(m.tokensIn)} / ${compactNumber(m.tokensOut)}` : 'unknown'}</Tile>
      <Tile label="Cost">{m.costUsd !== undefined ? `$${m.costUsd.toFixed(2)}` : 'unknown'}</Tile>
    </ul>
  );
}

type RowProps = { ctx: ControlCtx; onOutcome(o: Outcome | null): void; afterStop(): void };

/**
 * Start, Pause all, Resume all, Stop team… (the graceful stop: the project bar's confirmation) and
 * Emergency stop (`withStop`; a phone shows it on its one header line instead).
 */
function TeamControls({ ctx, onOutcome, afterStop, withStop = true }: RowProps & { withStop?: boolean }) {
  const t = ctx.source.team;
  const fix = ctx.startFix && <Link className="ctl-link" to={ctx.startFix.to}>{ctx.startFix.text}</Link>;
  return (
    <div className="ctl-controls" role="group" aria-label="Team controls">
      <ActionButton action={t.start} label="Start" glyph="▶" className="btn-primary primary" onOutcome={onOutcome} shortWhy={() => (ctx.source.running ? 'Running' : null)} fix={fix} />
      <ActionButton action={t.pauseAll} label="Pause all" glyph="‖" onOutcome={onOutcome} />
      <ActionButton action={t.resumeAll} label="Resume all" glyph="▶" onOutcome={onOutcome} />
      <ActionButton action={t.stop} label="Stop team…" glyph="◑" onOutcome={onOutcome} />
      {withStop && <EmergencyStop action={t.emergencyStop} onOutcome={onOutcome} afterStop={afterStop} />}
    </div>
  );
}

function Wide({ ctx, onOutcome, afterStop }: RowProps) {
  return (
    <div className="ctl-head-row">
      <Strip ctx={ctx} />
      <div className="ctl-head-right">
        <Caution ctx={ctx} />
        <TeamControls ctx={ctx} onOutcome={onOutcome} afterStop={afterStop} />
      </div>
    </div>
  );
}

/** A phone: one line (the run, the clock, the caution's count, Emergency stop); the rest one step back. */
function Narrow({ ctx, onOutcome, afterStop }: RowProps) {
  const run = runStatus(ctx.source);
  return (
    <>
      <div className="ctl-head-line">
        <span className={`ctl-line-run tone-${run.tone}`}><span aria-hidden="true">{run.glyph}</span> {run.word}</span>
        <Clock startedAt={ctx.source.startedAt} running={ctx.source.running} />
        <Caution ctx={ctx} />
        <EmergencyStop action={ctx.source.team.emergencyStop} onOutcome={onOutcome} afterStop={afterStop} />
      </div>
      <details className="ctl-head-more">
        <summary>Agents, tokens, cost and team controls</summary>
        <Strip ctx={ctx} />
        <TeamControls ctx={ctx} onOutcome={onOutcome} afterStop={afterStop} withStop={false} />
      </details>
    </>
  );
}

export function ControlHeader({ ctx }: { ctx: ControlCtx }) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const problems = ctx.source.problems ?? [];
  const Row = useMobile() ? Narrow : Wide;
  const note = useRef<HTMLDivElement>(null);
  // After Emergency stop its button is disabled (nothing runs): focus goes to what it did.
  const afterStop = () => requestAnimationFrame(() => note.current?.focus());
  return (
    <div className="ctl-head">
      <Row ctx={ctx} onOutcome={setOutcome} afterStop={afterStop} />
      <OutcomeNote ref={note} o={outcome} dismiss={() => setOutcome(null)} />
      {problems.length > 0 && (
        <ul className="ctl-problems" role="status" aria-label="Not loaded">
          {problems.map((p) => <li key={p}><span aria-hidden="true">◇ </span>{p}</li>)}
        </ul>
      )}
    </div>
  );
}
