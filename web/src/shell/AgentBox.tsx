import type { ReactNode } from 'react';
import { needsAttention } from '../status';
import { chipText, shortAge, tokensText, type ToolChip } from '../herdr';
import { WORDS } from '../vocab';
import type { AgentRow } from './model';
import { Btn, StateMark, StuckMark, VendorMark, WaitingMark } from './ui';
import { useSecondClock } from './secondClock';
import { CheckMark } from './CheckMark';

function Chip({ chip }: { chip: ToolChip }) {
  const now = useSecondClock(chip.status === 'running') || Date.now();
  return <span className={`chip chip-${chip.status}`}>{chipText(chip, now)}</span>;
}

/** Chips a card shows in its one chip row; the rest are counted as "+N". */
export const CARD_CHIPS = 2;

/** The card's chip row: the first CARD_CHIPS tools, and how many more the turn used. */
export function chipRow(chips: ToolChip[], toolCount: number, room = CARD_CHIPS): { shown: ToolChip[]; more: number } {
  const shown = chips.slice(0, room);
  return { shown, more: Math.max(0, toolCount - shown.length) };
}

function ChipRow({ chips, toolCount }: { chips: ToolChip[]; toolCount: number }) {
  const { shown, more } = chipRow(chips, toolCount);
  return (
    <>
      {shown.map((c) => (
        <Chip key={c.name} chip={c} />
      ))}
      {more > 0 && (
        <span className="chip chip-more" title={`${more} more tools this turn`}>
          +{more}
        </span>
      )}
    </>
  );
}

function BoxChipLine({ a, chips, waiting }: { a: AgentRow; chips: ToolChip[]; waiting: string[] }) {
  const stuck = a.state.key === 'stuck' ? a.stuck : undefined;
  if (stuck) return <StuckMark reason={stuck.reason} className="box-waiting" />;
  // Waiting for a handoff: no turn yet, so this line says what it waits for.
  if (waiting.length > 0) return <WaitingMark names={waiting} className="box-waiting" />;
  return <ChipRow chips={chips} toolCount={a.activity?.toolCount ?? chips.length} />;
}

/** The tool line shows only when there is something to say: stuck, waiting, or a tool. */
function BoxChips({ a }: { a: AgentRow }) {
  const live = a.seat?.lifecycle === 'running' || a.seat?.lifecycle === 'stale';
  const chips = live ? (a.activity?.chips ?? []) : [];
  const waiting = a.waitingFor ?? [];
  if (!(a.state.key === 'stuck' && a.stuck) && waiting.length === 0 && chips.length === 0) return null;
  return <span className="box-chips"><BoxChipLine a={a} chips={chips} waiting={waiting} /></span>;
}

/** The card's last line: why it needs you, a limit note, or the agent's last words. */
function BoxLast({ a }: { a: AgentRow }) {
  const seat = a.seat;
  const reason = seat && needsAttention(seat.lifecycle) ? seat.lifecycle_reason : null;
  const last = a.activity?.lastText?.split('\n').find((l) => l.trim()) ?? null;
  if (reason) {
    return (
      <span className="box-last reason clamp" data-raw-evidence data-raw-evidence-title="" title={reason}>
        {reason}
      </span>
    );
  }
  if (a.activity?.limitNote) {
    return (
      <span className="box-last tone-amber" data-raw-evidence>
        {a.activity.limitNote}
      </span>
    );
  }
  return (
    <span className="box-last" data-raw-evidence>
      {last ? `› ${last}` : '› no reply yet'}
    </span>
  );
}

function BoxFoot({ a, now, onAnswer }: { a: AgentRow; now: number; onAnswer: () => void }) {
  return (
    <div className="box-foot">
      <span className="box-foot-text">
        {a.state.key === 'needs-you' ? '' : `${a.vendor.name} · `}
        {tokensText(a.seat)}
      </span>
      <span className="box-age" title="Last activity">{shortAge(a.lastMs, now)}</span>
      {a.state.key === 'needs-you' && (
        <Btn variant="danger" size="sm" icon="message" className="box-answer" onClick={onAnswer} aria-label={`Answer ${a.name}`} tip="Open the agent and answer it">
          Answer
        </Btn>
      )}
    </div>
  );
}

function boxLabel(a: AgentRow) {
  if (!a.seat) return WORDS.unknownAgent(a.name);
  const stuck = a.state.key === 'stuck' ? a.stuck : undefined;
  return `${a.name} — ${a.state.word}${stuck || (a.waitingFor ?? []).length ? `, ${a.summary}` : ''}`;
}

type BoxProps = {
  a: AgentRow;
  selected: boolean;
  /** One of several agents picked to message together. */
  picked?: boolean;
  onOpen: () => void;
  onAnswer: () => void;
  onFocusBox?: () => void;
  now: number;
  /** The components this agent uses, as chips. */
  components?: ReactNode;
};

export function AgentBox({ a, selected, picked = false, onOpen, onAnswer, onFocusBox, now, components }: BoxProps) {
  const seat = a.seat;
  return (
    <div className={`agent-box ${a.vendor.cls} st-${a.state.key} tone-box-${a.state.tone}${selected ? ' selected' : ''}${picked ? ' picked' : ''}`}>
      <button type="button" className="box-main" aria-label={boxLabel(a)} aria-pressed={selected} data-agent={a.id} onClick={onOpen} onFocus={onFocusBox}>
        <span className="box-head">
          <VendorMark v={a.vendor} size="md" />
          <span className="box-name">{a.name}</span>
        </span>
        <span className="box-state">
          <StateMark state={a.state} pill />
          {a.state.key === 'working' && seat?.current_tool && <span className="box-tool"> · {seat.current_tool.toLowerCase()}</span>}
          {seat?.check && <CheckMark check={seat.check} />}
        </span>
        {components}
        <BoxChips a={a} />
        <BoxLast a={a} />
      </button>
      <BoxFoot a={a} now={now} onAnswer={onAnswer} />
    </div>
  );
}
