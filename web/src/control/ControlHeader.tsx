import { useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { StatusTile as Tile } from '../components/StatusTile';
import { compactNumber } from '../format';
import { useMobile } from '../workspace/MobileWorkspace';
import { ActionButton, OutcomeNote, type Outcome } from './ActionButton';
import { span, type ControlCtx } from './context';
import { EmergencyStop } from './EmergencyStop';
import { countsOf, runStatus } from './runStatus';
import type { TeamActions } from './source';

/**
 * Control's header: a status strip read at a glance (run status, elapsed clock, agents by status,
 * tokens and cost, the master caution), then the whole team's controls. Every reading is a glyph
 * and a word or a number; anything not known says "unknown".
 */

/**
 * T+hh:mm:ss since the earliest current session started, ticking each second while the team runs.
 * A run that has finished (`endedAt`, a simulated branch) shows how long it ran.
 */
function Clock({ startedAt, endedAt, running, unread }: { startedAt?: number; endedAt?: number; running: boolean; unread?: boolean }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running || startedAt === undefined) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [running, startedAt]);
  if (unread) return <><span className="ctl-clock is-quiet">T+ --:--:--</span> <small>unknown</small></>;
  if (!running && startedAt !== undefined && endedAt !== undefined) return <><span className="ctl-clock is-quiet">{span(endedAt - startedAt)}</span> <small>ran for</small></>;
  if (!running || startedAt === undefined) return <><span className="ctl-clock is-quiet">T+ --:--:--</span> <small>{running ? 'unknown' : 'not running'}</small></>;
  return <span className="ctl-clock" role="timer" aria-label={`Running for ${span(now - startedAt)}`}>T+{span(now - startedAt)}</span>;
}

/** Tokens in / out as reported, else the total where only that was reported (an Arena run), else unknown. */
function tokenWords(m: ControlCtx['source']['metrics']): string {
  if (m.tokensIn !== undefined && m.tokensOut !== undefined) return `${compactNumber(m.tokensIn)} / ${compactNumber(m.tokensOut)}`;
  return m.tokens !== undefined ? `${compactNumber(m.tokens)} total` : 'unknown';
}

/** Lit while some alarm is not acknowledged; "unknown" while nothing about the run could be read. */
function Caution({ ctx }: { ctx: ControlCtx }) {
  const { unacked, list } = ctx.alarms;
  const lit = unacked > 0;
  const unread = !!ctx.source.unread;
  const words = unread ? 'unknown' : lit ? `${unacked} not acknowledged` : list.length > 0 ? `${list.length} acknowledged` : 'no alarms';
  return (
    <button type="button" className={`ctl-caution${lit ? ' is-lit' : ''}`} onClick={() => ctx.show('alarms')} aria-label={`Master caution: ${words}. Open Alarms`}>
      <span className="ctl-caution-glyph" aria-hidden="true">{unread ? '∅' : lit ? '▲' : list.length > 0 ? '✓' : '○'}</span>
      <span className="ctl-caution-word">Master caution</span>
      <span className="ctl-caution-count">{unread ? '—' : lit ? unacked : 0}</span>
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
      <Tile label="Run" tone={run.tone} glyph={run.glyph}>{run.word}</Tile>
      <Tile label="Elapsed"><Clock startedAt={source.startedAt} endedAt={source.endedAt} running={source.running} unread={source.unread} /></Tile>
      <Tile label="Agents" className="ctl-tile-wide">
        {source.unread ? 'unknown' : counts.length === 0 ? 'none yet' : counts.map((c) => <span key={c.word} className={`ctl-count tone-${c.tone}`}><span className="ctl-glyph" aria-hidden="true">{c.glyph}</span> {c.n} {c.word}</span>)}
      </Tile>
      <Tile label={m.tokensIn === undefined && m.tokens !== undefined ? 'Tokens' : 'Tokens in / out'}>{tokenWords(m)}</Tile>
      <Tile label="Cost">{m.costUsd !== undefined ? `$${m.costUsd.toFixed(2)}` : 'unknown'}</Tile>
    </ul>
  );
}

type RowProps = { ctx: ControlCtx; onOutcome(o: Outcome | null): void; afterStop(): void };

/** The reason Pause all, Resume all, Stop team… and Emergency stop share, when they share one (an Arena run). */
function sharedWhy(t: TeamActions): string | null {
  const why = [t.pauseAll, t.resumeAll, t.stop, t.emergencyStop].map((a) => (a.can.ok ? null : a.can.why));
  return why[0] && why.every((w) => w === why[0]) ? why[0] : null;
}

/**
 * Start, Pause all, Resume all, Stop team… (the graceful stop: the project bar's confirmation) and
 * Emergency stop (`withStop`; a phone shows it on its one header line instead). A reason the stop
 * controls share is said once, after them (each keeps it as its description); on a phone, Emergency
 * stop's own line already says it.
 */
function TeamControls({ ctx, onOutcome, afterStop, withStop = true }: RowProps & { withStop?: boolean }) {
  const t = ctx.source.team;
  const fix = ctx.startFix && <Link className="ctl-link" to={ctx.startFix.to}>{ctx.startFix.text}</Link>;
  const shared = sharedWhy(t);
  const short = shared ? () => '' : undefined;
  return (
    <div className="ctl-controls" role="group" aria-label="Team controls">
      <ActionButton action={t.start} label="Start" glyph="▶" className="btn-primary primary" onOutcome={onOutcome} shortWhy={() => (ctx.source.running ? 'Running' : null)} fix={fix} />
      <ActionButton action={t.pauseAll} label="Pause all" glyph="‖" onOutcome={onOutcome} shortWhy={short} />
      <ActionButton action={t.resumeAll} label="Resume all" glyph="▶" onOutcome={onOutcome} shortWhy={short} />
      <ActionButton action={t.stop} label="Stop team…" glyph="◑" onOutcome={onOutcome} shortWhy={short} />
      {withStop && <EmergencyStop action={t.emergencyStop} onOutcome={onOutcome} afterStop={afterStop} shortWhy={short} />}
      {shared && withStop && <p className="ctl-why ctl-why-shared"><span aria-hidden="true">◇ </span>{shared}</p>}
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
        <Clock startedAt={ctx.source.startedAt} endedAt={ctx.source.endedAt} running={ctx.source.running} unread={ctx.source.unread} />
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
