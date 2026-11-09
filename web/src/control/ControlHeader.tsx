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
 * usage, the master caution), then the whole team's controls. Every reading is a glyph
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
  if (!running || startedAt === undefined) return <><span className="ctl-clock is-quiet">T+ --:--:--</span>{running && <> <small>unknown</small></>}</>;
  return <span className="ctl-clock" role="timer" aria-label={`Running for ${span(now - startedAt)}`}>T+{span(now - startedAt)}</span>;
}

/** Tokens in / out as reported, else the total where only that was reported (an Arena run); null when not reported. */
function tokenWords(m: ControlCtx['source']['metrics']): string | null {
  if (m.tokensIn !== undefined && m.tokensOut !== undefined) return `${compactNumber(m.tokensIn)} in / ${compactNumber(m.tokensOut)} out`;
  return m.tokens !== undefined ? `${compactNumber(m.tokens)} total` : null;
}

/** Tokens and cost in one reading: "unknown" once when neither was reported. */
function Usage({ m }: { m: ControlCtx['source']['metrics'] }) {
  const tokens = tokenWords(m);
  const cost = m.costUsd !== undefined ? `$${m.costUsd.toFixed(2)}` : null;
  if (!tokens && !cost) return <>unknown</>;
  return <><span>{tokens ?? 'tokens unknown'}</span> <span>{cost ?? <small>cost unknown</small>}</span></>;
}

/** The caution in words: not acknowledged, else acknowledged and shelved, else none. */
function cautionWords({ unacked, list, shelved }: ControlCtx['alarms'], unread: boolean): string {
  if (unread) return 'unknown';
  if (unacked > 0) return `${unacked} not acknowledged`;
  if (list.length === 0) return 'no alarms';
  const off = list.filter((a) => shelved.has(a.id)).length;
  return [list.length - off ? `${list.length - off} acknowledged` : '', off ? `${off} shelved` : ''].filter(Boolean).join(', ');
}

/** Lit while some alarm is neither acknowledged nor shelved; "unknown" while nothing about the run could be read. */
function Caution({ ctx }: { ctx: ControlCtx }) {
  const { unacked, list } = ctx.alarms;
  const lit = unacked > 0;
  const unread = !!ctx.source.unread;
  const words = cautionWords(ctx.alarms, unread);
  return (
    <button type="button" className={`ctl-caution${lit ? ' is-lit' : ''}`} onClick={() => ctx.show('alarms')} aria-label={`Master caution: ${words}. Open Alarms`}>
      <span className="ctl-caution-glyph" aria-hidden="true">{unread ? '∅' : lit ? '▲' : list.length > 0 ? '✓' : '○'}</span>
      <span className="ctl-caution-word">Master caution</span>
      <span className="ctl-caution-count">{unread ? '—' : lit ? unacked : 0}</span>
      {words !== 'no alarms' && <span className="ctl-caution-sub">{words}</span>}
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
      <Tile label="Usage"><Usage m={m} /></Tile>
    </ul>
  );
}

type RowProps = { ctx: ControlCtx; onOutcome(o: Outcome | null): void; afterStop(): void };

/** The reason most of `actions` share while they cannot run (each keeps its own as its tooltip). */
function commonWhy(actions: TeamActions[keyof TeamActions][]): string | null {
  const count = new Map<string, number>();
  for (const a of actions) if (!a.can.ok) count.set(a.can.why, (count.get(a.can.why) ?? 0) + 1);
  return [...count].sort((x, y) => y[1] - x[1])[0]?.[0] ?? null;
}

/**
 * Start (the one primary button), Pause all, Resume all, Stop team… (the graceful stop: the project
 * bar's confirmation) and Emergency stop (`withStop`; a phone shows it on its one header line
 * instead). Their reasons are said once, in one line under them: why Start cannot run (with where to
 * fix it), then the reason the others share. Each button keeps its own as tooltip and description.
 */
function TeamControls({ ctx, onOutcome, afterStop, withStop = true }: RowProps & { withStop?: boolean }) {
  const t = ctx.source.team;
  const startWhy = !t.start.can.ok && !ctx.source.running ? t.start.can.why : null;
  const rest = commonWhy([t.pauseAll, t.resumeAll, t.stop, ...(withStop ? [t.emergencyStop] : [])]);
  return (
    <div className="ctl-controls" role="group" aria-label="Team controls">
      <div className="ctl-row">
        <ActionButton action={t.start} label="Start" glyph="▶" className="btn-primary primary" onOutcome={onOutcome} quiet />
        <ActionButton action={t.pauseAll} label="Pause all" glyph="‖" onOutcome={onOutcome} quiet />
        <ActionButton action={t.resumeAll} label="Resume all" glyph="▶" onOutcome={onOutcome} quiet />
        <ActionButton action={t.stop} label="Stop team…" glyph="◑" onOutcome={onOutcome} quiet />
        {withStop && <EmergencyStop action={t.emergencyStop} onOutcome={onOutcome} afterStop={afterStop} quiet />}
      </div>
      {(startWhy || rest) && (
        <p className="ctl-why ctl-why-line">
          {startWhy && <span>{startWhy}</span>}
          {startWhy && ctx.startFix && <Link className="ctl-link" to={ctx.startFix.to}>{ctx.startFix.text}</Link>}
          {rest && <span>{rest}</span>}
        </p>
      )}
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
