import { PhaseBrief, workMessage } from './PhaseBrief';
import type { KeyboardEvent, ReactNode } from 'react';
import { useRef, useState } from 'react';
import { durationText } from '../herdr';
import { tokenSum, tokensLabel } from '../format';
import { KIND_ICON } from '../shell/ToolCard';
import { FoldToggle, isLong, useFold, type FoldLimit } from '../shell/Fold';
import { Icon, type IconName } from '../shell/ui';
import type { Phase, Step, Trigger } from './process';
import { KIND_WORD, TOOL_KINDS } from './tools';
import { PHASE_WORD, plainLine, Running, STEP_WORD } from './visualParts';

function triggerText(t: Trigger): { label: string; text: string } {
  switch (t.kind) {
    case 'job':
      return { label: 'Job · sent at start', text: plainLine(workMessage(t.text).split('\n')[0]) };
    case 'handoff':
      return { label: t.firstLine || 'Handed over', text: '' };
    case 'agent':
      return {label: `Agent ${t.from.join(', ')}:`, text: plainLine(workMessage(t.text).split('\n')[0])};
    case 'system':
      return {label: 'System:', text: plainLine(workMessage(t.text).split('\n')[0])};
    default:
      return { label: 'You:', text: plainLine(workMessage(t.text).split('\n')[0]) };
  }
}

const TRIGGER_ICON: Record<Trigger['kind'], IconName> = { job: 'file-text', handoff: 'arrow-right', operator: 'message', agent: 'message', system: 'message' };

const NODES = 40;

/** Arrow keys, Home and End move between a phase's nodes (one tab stop); returns the focused index. */
function nextNode(key: string, i: number, count: number): number | null {
  switch (key) {
    case 'ArrowRight': case 'ArrowDown': return Math.min(count - 1, i + 1);
    case 'ArrowLeft': case 'ArrowUp': return Math.max(0, i - 1);
    case 'Home': return 0;
    case 'End': return count - 1;
    default: return null;
  }
}

function NodeButton({ s, i, active, setActive, onJump }: { s: Step; i: number; active: boolean; setActive: (i: number) => void; onJump: (key: string) => void }) {
  const label = `${s.name}: ${s.summary} — ${STEP_WORD[s.status]}`;
  return (
    <button
      type="button"
      className={`vp-node k-${s.kind} s-${s.status}${s.helper ? ' helper' : ''}`}
      tabIndex={active ? 0 : -1}
      aria-label={label}
      data-tip={label}
      data-raw-evidence-title
      onFocus={() => setActive(i)}
      onClick={() => onJump(s.key)}
    >
      <Icon name={KIND_ICON[s.kind]} size={12} />
      {s.status === 'error' && (
        <span className="vp-node-x" aria-hidden="true">
          ✗
        </span>
      )}
    </button>
  );
}

/** A phase's tool calls as small nodes: one tab stop, arrow keys move between them. */
function NodeRow({ phase, onJump }: { phase: Phase; onJump: (key: string) => void }) {
  const [active, setActive] = useState(0);
  const [all, setAll] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const steps = all ? phase.steps : phase.steps.slice(0, NODES);
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const nodes = [...(ref.current?.querySelectorAll<HTMLButtonElement>('button.vp-node') ?? [])];
    const i = nodes.indexOf(document.activeElement as HTMLButtonElement);
    const next = i < 0 ? null : nextNode(e.key, i, nodes.length);
    if (next === null) return;
    e.preventDefault();
    setActive(next);
    nodes[next].focus();
  };
  if (phase.steps.length === 0) return <p className="vp-nodes-empty muted">No tool calls</p>;
  return (
    <div className="vp-nodes-wrap">
      <div className="vp-nodes" role="group" aria-label={`Turn ${phase.n} tool calls`} ref={ref} onKeyDown={onKeyDown}>
        {steps.map((s, i) => <NodeButton key={s.key} s={s} i={i} active={i === Math.min(active, steps.length - 1)} setActive={setActive} onJump={onJump} />)}
      </div>
      {phase.steps.length > NODES && (
        <button type="button" className="link-btn" onClick={() => setAll((v) => !v)}>
          {all ? 'Show fewer' : `+${phase.steps.length - NODES} more`}
        </button>
      )}
    </div>
  );
}

function Counts({ phase }: { phase: Phase }) {
  const kinds = TOOL_KINDS.filter((k) => phase.counts[k]);
  if (kinds.length === 0) return null;
  return (
    <p className="vp-counts">
      {kinds.map((k, i) => (
        <span key={k} className={`k-${k}`}>
          {i > 0 && <span className="sep">·</span>}
          {KIND_WORD[k]} {phase.counts[k]}
        </span>
      ))}
      {phase.failedSteps > 0 && <span className="tone-red"> · ✗ {phase.failedSteps} failed</span>}
    </p>
  );
}

/** Seconds under a minute, then minutes, then hours and minutes. */
function span(ms: number): string {
  if (ms < 60_000) return durationText(ms);
  const m = Math.floor(ms / 60_000);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

function phaseMeta(p: Phase, now: number): string {
  const dur = p.endMs !== null ? span(p.endMs - p.startMs) : p.status === 'running' ? `for ${span(Math.max(0, now - p.startMs))}` : 'duration unknown';
  return `${dur} · ${tokensLabel(tokenSum(p.tokens))}`;
}

/** Turn text past one line or this many characters starts clamped, with "Show more" on the card. */
const CARD_FOLD: FoldLimit = { chars: 160, lines: 1 };

function PhaseHead({ p, now, children }: { p: Phase; now: number; children?: ReactNode }) {
  return (
    <div className="vp-card-head">
      <span className="vp-turn">{p.turnId === null ? 'Outside a turn' : `Turn ${p.n}`}</span>{' '}
      <span className={`vp-status st-${p.status}`} title="How the turn ended. It says nothing about whether the work is right.">
        {p.status === 'running' && <Running />}
        {PHASE_WORD[p.status]}
      </span>{' '}
      <span className="vp-meta">{phaseMeta(p, now)}</span>
      {children}
    </div>
  );
}

/** What started the turn: its first line, clamped; the whole message when the card is open. */
function TriggerRow({ p, open, onJump }: { p: Phase; open: boolean; onJump: (key: string) => void }) {
  const t = p.triggers[0];
  if (!t) return <p className="vp-trigger none">Started by: unknown</p>;
  const tt = triggerText(t);
  const text = open ? t.text : tt.text;
  return (
    <button type="button" className={`vp-trigger o-${t.kind}`} onClick={() => onJump(t.key)}>
      <Icon name={TRIGGER_ICON[t.kind]} size={13} />
      <span className="vp-trigger-text">
        <span className="vp-trigger-label">{tt.label}</span>
        {p.triggers.length > 1 && <span className="muted"> +{p.triggers.length - 1} more</span>}{' '}
        {text && <span data-raw-evidence>{text}</span>}
      </span>
    </button>
  );
}

/** The turn's last reply (◇ reported): its first line, clamped; all of it when the card is open. */
function ReplyRow({ reply, open, onJump }: { reply: NonNullable<Phase['reply']>; open: boolean; onJump: (key: string) => void }) {
  return (
    <button type="button" className="vp-reply" onClick={() => onJump(reply.key)}>
      <span className="vp-reply-text">
        <span className="tier-inline tier-reported">◇ reported</span>{' '}
        <span data-raw-evidence title={open ? undefined : reply.firstLine} data-raw-evidence-title>
          {open ? reply.text : plainLine(reply.firstLine)}
        </span>
      </span>
    </button>
  );
}

const foldsInCard = (p: Phase) => isLong(p.triggers[0]?.text ?? '', CARD_FOLD) || isLong(p.reply?.text ?? '', CARD_FOLD);

export function PhaseCard({ p, onJump, now, last }: { p: Phase; onJump: (key: string) => void; now: number; last: boolean }) {
  const { open, toggle } = useFold();
  return (
    <li className={`vp-phase st-${p.status}${last ? ' last' : ''}`} data-phase={p.n}>
      <div className="vp-rail" aria-hidden="true">
        <span className="vp-dot">{p.status === 'running' ? <Running /> : p.n}</span>
        <span className="vp-line" />
      </div>
      <div className="vp-card" data-open={open}>
        <PhaseHead p={p} now={now}>
          {foldsInCard(p) && <FoldToggle open={open} onToggle={toggle} className="vp-more" />}
        </PhaseHead>
        <TriggerRow p={p} open={open} onJump={onJump} />
        <PhaseBrief phase={p} onJump={onJump} />
        <NodeRow phase={p} onJump={onJump} />
        <Counts phase={p} />
        {p.reply && <ReplyRow reply={p.reply} open={open} onJump={onJump} />}
      </div>
    </li>
  );
}

export function Waiting({ items, onJump }: { items: Trigger[]; onJump: (key: string) => void }) {
  if (items.length === 0) return null;
  return (
    <div className="vp-waiting">
      {items.map((w) => (
        <button key={w.key} type="button" className={`vp-wait${w.state === 'failed' ? ' failed' : ''}`} onClick={() => onJump(w.key)}>
          {w.state === 'failed' ? <span className="tone-red">✗ not sent</span> : <span className="muted">◷ waiting to be read</span>}
          <span className="vp-trigger-label">{triggerText(w).label}</span>
          {triggerText(w).text && <span data-raw-evidence>{triggerText(w).text}</span>}
          {w.reason && (
            <span className="muted">
              {' — '}
              <span data-raw-evidence>{w.reason}</span>
            </span>
          )}
        </button>
      ))}
    </div>
  );
}

export function ProcessList({ phases, onJump }: { phases: Phase[]; onJump: (key: string) => void }) {
  return (
    <ol className="vp-list" aria-label="Process as a list">
      {phases.map((p) => (
        <li key={p.id}>
          <div className="vp-list-head">
            {p.turnId === null ? 'Outside a turn' : `Turn ${p.n}`} — {PHASE_WORD[p.status]}
          </div>
          <ul>
            {p.triggers.map((t) => (
              <li key={t.key}>
                <button type="button" className="link-btn" onClick={() => onJump(t.key)}>
                  Start: {triggerText(t).label} <span data-raw-evidence>{triggerText(t).text}</span>
                </button>
              </li>
            ))}
            {p.steps.map((s) => (
              <li key={s.key}>
                <button type="button" className={`link-btn s-${s.status}`} onClick={() => onJump(s.key)}>
                  <span data-raw-evidence>
                    {s.name} {s.summary}
                  </span>{' '}
                  — {STEP_WORD[s.status]}
                </button>
              </li>
            ))}
            {p.reply && (
              <li>
                <button type="button" className="link-btn" onClick={() => onJump(p.reply!.key)}>
                  Reply (◇ reported): <span data-raw-evidence>{plainLine(p.reply.firstLine)}</span>
                </button>
              </li>
            )}
          </ul>
        </li>
      ))}
    </ol>
  );
}

