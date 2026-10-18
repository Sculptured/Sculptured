import type { ReactNode } from 'react';
import type { Vendor } from '../herdr';
import { WORDS } from '../vocab';
import type { AgentRow } from './model';
import { Icon, StateMark } from './ui';

/** Ray lengths of the Claude starburst, going round from the top: uneven, like a hand-cut spark. */
const RAYS = [19, 15, 18, 14.5, 19.5, 15.5, 18.5, 14, 19, 16, 18, 15];

/**
 * The Claude starburst: the app's asterisk mark (`claude` in icons.tsx) drawn out to twelve
 * uneven rays. Drawn here; no outside artwork.
 */
function ClaudeBurst() {
  return (
    <svg className="logo-svg" viewBox="0 0 48 48" width="52" height="52" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" strokeWidth="3.6" strokeLinecap="round">
      {RAYS.map((r, i) => {
        const a = (i / RAYS.length) * 2 * Math.PI - Math.PI / 2;
        const f = (n: number) => Math.round(n * 100) / 100;
        return <line key={i} x1={f(24 + Math.cos(a) * 3)} y1={f(24 + Math.sin(a) * 3)} x2={f(24 + Math.cos(a) * r)} y2={f(24 + Math.sin(a) * r)} />;
      })}
    </svg>
  );
}

/** The provider's mark, large: Claude's starburst, the Codex mark, the Practice flask, or a neutral bot. */
export function BrandLogo({ v }: { v: Vendor }) {
  if (v.cls === 'v-claude') return <ClaudeBurst />;
  if (v.cls === 'v-codex') return <Icon name="codex" size={50} strokeWidth={1.7} className="logo-svg" />;
  if (v.cls === 'v-practice') return <Icon name="practice" size={46} strokeWidth={1.7} className="logo-svg" />;
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
        {v.cls === 'v-practice' && (
          <span className="logo-bubbles" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
        )}
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
