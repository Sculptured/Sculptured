import type { ReactNode } from 'react';
import type { Vendor } from '../herdr';
import { WORDS } from '../vocab';
import type { AgentRow } from './model';
import { Icon, StateMark } from './ui';

/**
 * The Claude Code pixel guy on an 18 × 10 grid of tall pixels (1 wide, 2 high), read off the block
 * art Claude Code prints at start: ` ▐▛███▜▌` / `▝▜█████▛▘` / `  ▘▘ ▝▝`. Body with its arms and
 * two eye holes, then the legs in two pairs that step in turn while it works. Drawn here; no
 * outside artwork. `claude` in icons.tsx is the same shape at icon size.
 */
const CLAWD_BODY = 'M3 0H15V4H17V6H15V8H3V6H1V4H3ZM5 2H6V4H5ZM12 2H13V4H12Z';
const CLAWD_LEGS_A = 'M4 8h1v2h-1ZM11 8h1v2h-1Z';
const CLAWD_LEGS_B = 'M6 8h1v2h-1ZM13 8h1v2h-1Z';

function ClaudeGuy() {
  return (
    <svg className="logo-svg clawd" viewBox="0 -1 18 12" width="66" height="44" aria-hidden="true" focusable="false" fill="currentColor" shapeRendering="crispEdges">
      <path className="clawd-body" fillRule="evenodd" d={CLAWD_BODY} />
      <path className="clawd-legs-a" d={CLAWD_LEGS_A} />
      <path className="clawd-legs-b" d={CLAWD_LEGS_B} />
    </svg>
  );
}

/** The provider's mark, large: the Claude Code guy, the Codex mark, or a bot (Practice agents in their own colour). */
export function BrandLogo({ v }: { v: Vendor }) {
  if (v.cls === 'v-claude') return <ClaudeGuy />;
  if (v.cls === 'v-codex') return <Icon name="codex" size={50} strokeWidth={1.7} className="logo-svg" />;
  if (v.cls === 'v-practice') return <Icon name="practice" size={44} strokeWidth={1.7} className="logo-svg" />;
  return <Icon name={v.name === 'unknown' ? 'unknown' : 'bot'} size={44} strokeWidth={1.7} className="logo-svg" />;
}

/**
 * A logo node's face: the mark on a quiet disc, the name under it, then the status as a shape
 * and words. `badge` sits on the disc's corner (needs you, things to fix).
 */
export function LogoFace({ v, name, status, badge, extra }: { v: Vendor; name: string; status: ReactNode; badge?: ReactNode; extra?: ReactNode }) {
  return (
    <>
      <span className="logo-disc">
        <span className="logo-orbit" aria-hidden="true" />
        <span className="logo-mark" aria-hidden="true">
          <span className="logo-turn">
            <BrandLogo v={v} />
          </span>
        </span>
        {badge}
      </span>
      <span className="logo-name" title={name}>{name}</span>
      {status}
      {extra}
    </>
  );
}

/** The classes every logo node shares: which mark, its state, and whether it moves. */
export function logoClasses(v: Vendor, working: boolean, motion: boolean): string {
  return `is-logo ${v.cls}${working ? ' logo-working' : ''}${working && motion ? ' logo-animated' : ''}`;
}

/**
 * An agent on the Run board in Logos style: same slot, same button, same name for screen readers
 * as its card. While it works its logo moves (unless motion is reduced or the tab is hidden).
 */
type LogoBoxProps = {
  a: AgentRow;
  selected: boolean;
  picked?: boolean;
  onOpen: () => void;
  onFocusBox?: () => void;
  /** Working logos may move (see `useLogoMotion`). */
  motion: boolean;
  /** The components this agent uses, as chips. */
  chips?: ReactNode;
};

/** The agent's name for screen readers, as on its card: name, state, and why it is stuck or waiting. */
function boxLabel(a: AgentRow): string {
  if (!a.seat) return WORDS.unknownAgent(a.name);
  const extra = (a.state.key === 'stuck' && a.stuck) || (a.waitingFor ?? []).length ? `, ${a.summary}` : '';
  return `${a.name} — ${a.state.word}${extra}`;
}

export function LogoBox({ a, selected, picked = false, onOpen, onFocusBox, motion, chips }: LogoBoxProps) {
  const label = boxLabel(a);
  const needsYou = a.state.key === 'needs-you';
  return (
    <div className={`agent-box ${logoClasses(a.vendor, a.state.key === 'working', motion)} st-${a.state.key}${selected ? ' selected' : ''}${picked ? ' picked' : ''}`}>
      <button type="button" className="box-main logo-main" aria-label={label} aria-pressed={selected} data-agent={a.id} onClick={onOpen} onFocus={onFocusBox}>
        <LogoFace
          v={a.vendor}
          name={a.name}
          status={<StateMark state={a.state} pill className="logo-status" />}
          badge={needsYou ? <span className="logo-badge tone-red" aria-hidden="true">!</span> : undefined}
          extra={chips}
        />
      </button>
    </div>
  );
}
