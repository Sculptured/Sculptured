import { useEffect, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from 'react';
import type { AgentState, Vendor } from '../herdr';
import { Icon, type IconName } from './icons';

export { Icon, type IconName } from './icons';

export type BtnVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** Destructive actions: red text on a subtle red fill. Same as `variant="danger"`. */
  danger?: boolean;
  /** A key that does the same thing. Shown in the tooltip and exposed as `aria-keyshortcuts`. */
  kbd?: string;
  pressed?: boolean;
  ref?: React.Ref<HTMLButtonElement>;
  /** Visual weight. Defaults to secondary (a subtle filled button with a 1px border). */
  variant?: BtnVariant;
  size?: 'sm' | 'md';
  /** An icon before the label. */
  icon?: IconName;
  /** Only the icon shows; `aria-label` is the name and the tooltip. */
  iconOnly?: boolean;
  /** Tooltip text. Defaults to `title`, then `aria-label` for icon-only buttons. */
  tip?: string;
};

/**
 * A real button: primary (filled accent), secondary (subtle fill, 1px border), ghost
 * (transparent until hovered) or danger (red text on a subtle fill). Each has hover, active
 * and visible focus states. A `kbd` shortcut shows in the tooltip, not in the label.
 */
export function Btn({ danger, kbd, pressed, className, children, variant, size, icon, iconOnly, tip, title, ...rest }: BtnProps) {
  const v: BtnVariant = variant ?? (danger ? 'danger' : 'secondary');
  const label = rest['aria-label'];
  const tipText = tip ?? title ?? (iconOnly ? label : undefined);
  return (
    <button
      type="button"
      className={`btn btn-${v}${v === 'primary' || v === 'danger' ? ` ${v}` : ''}${size === 'sm' ? ' btn-sm' : ''}${iconOnly ? ' btn-icon' : ''}${className ? ` ${className}` : ''}`}
      aria-pressed={pressed}
      aria-keyshortcuts={kbd ? kbdAria(kbd) : undefined}
      data-tip={tipText || kbd ? (tipText ?? '') : undefined}
      data-kbd={kbd}
      {...rest}
    >
      {icon && <Icon name={icon} size={size === 'sm' ? 14 : 15} />}
      {iconOnly ? null : children}
    </button>
  );
}

/** `esc` → `Escape`, `f` → `f`, `^z` → `Control+z`: the attribute's key names. */
function kbdAria(k: string): string {
  const map: Record<string, string> = { esc: 'Escape', '⏎': 'Enter', del: 'Delete' };
  return k
    .split(/\s*·\s*/)
    .map((part) => map[part] ?? part.replace(/^\^/, 'Control+').replace(/^Ctrl\+/, 'Control+'))
    .join(' ');
}

/** A rounded panel with a title row: a heading on the left, actions on the right. */
export function Panel({
  title,
  tools,
  className,
  children,
  label,
}: {
  title: ReactNode;
  tools?: ReactNode;
  className?: string;
  children: ReactNode;
  label?: string;
}) {
  return (
    <section className={`panel${className ? ` ${className}` : ''}`} aria-label={label}>
      <div className="panel-bar">
        <span className="panel-title">{title}</span>
        <span className="panel-rule" aria-hidden="true" />
        {tools && <span className="panel-tools">{tools}</span>}
      </div>
      {children}
    </section>
  );
}

/**
 * Tabs or a segmented control: one selected pill in a tray. Each option is a button with
 * `aria-pressed`, so it reads as a toggle group.
 */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  className,
}: {
  label: string;
  value: T;
  options: { value: T; label: string; icon?: IconName; kbd?: string; tip?: string }[];
  onChange: (v: T) => void;
  className?: string;
}) {
  return (
    <span role="group" aria-label={label} className={`seg${className ? ` ${className}` : ''}`}>
      {options.map((o) => (
        <Btn key={o.value} variant="ghost" size="sm" icon={o.icon} pressed={value === o.value} kbd={o.kbd} tip={o.tip} onClick={() => onChange(o.value)}>
          {o.label}
        </Btn>
      ))}
    </span>
  );
}

export type BadgeTone = 'neutral' | 'accent' | 'red' | 'green' | 'amber' | 'violet' | 'vendor';

/** A small rounded label: a count, a kind, a short status word. */
export function Badge({ tone = 'neutral', children, className, title }: { tone?: BadgeTone; children: ReactNode; className?: string; title?: string }) {
  return (
    <span className={`badge badge-${tone}${className ? ` ${className}` : ''}`} title={title}>
      {children}
    </span>
  );
}

/**
 * Status: the state's glyph (its shape) plus its word, never colour alone. `pill` sets it in a
 * tinted rounded pill. The working glyph is drawn as a spinning ring unless motion is reduced.
 */
export function StateMark({ state, word = true, className, pill }: { state: AgentState; word?: boolean; className?: string; pill?: boolean }) {
  return (
    <span className={`state tone-${state.tone} st-${state.key}${pill ? ' pill' : ''}${className ? ` ${className}` : ''}`} data-tip={state.tip}>
      <span className={`glyph${state.key === 'working' ? ' spin' : ''}`} aria-hidden="true">
        {state.glyph}
      </span>
      {word && <span className="word">{state.word}</span>}
    </span>
  );
}

/** The clock glyph that marks an agent waiting for a handoff. */
export const WAITING_GLYPH = '◷';

/**
 * "◷ waiting for Planner, Builder 1": the agent starts its work when these agents finish and
 * hand their results over. Glyph plus words, like every status.
 */
export function WaitingMark({ names, className }: { names: string[]; className?: string }) {
  return (
    <span
      className={`waiting-mark${className ? ` ${className}` : ''}`}
      data-tip={`Waiting for ${names.join(', ')}: starts when ${names.length > 1 ? 'they have' : 'it has'} finished, and ${names.length > 1 ? 'their results are' : 'the result is'} passed along`}
    >
      <span className="glyph" aria-hidden="true">
        {WAITING_GLYPH}
      </span>
      <span className="word">waiting for {names.join(', ')}</span>
    </span>
  );
}

/**
 * "▲ stuck: Airframe needs attention", in red: the agent's handoffs cannot go on by
 * themselves. Glyph plus words, like every status.
 */
export function StuckMark({ reason, className }: { reason: string; className?: string }) {
  return (
    <span className={`stuck-mark tone-red${className ? ` ${className}` : ''}`} data-tip={`Stuck: ${reason}`}>
      <span className="glyph" aria-hidden="true">
        ▲
      </span>
      <span className="word">stuck: {reason}</span>
    </span>
  );
}

const VENDOR_ICON: Record<string, IconName> = { 'v-claude': 'claude', 'v-codex': 'codex', 'v-opencode': 'opencode', 'v-practice': 'practice' };

/**
 * The agent's avatar: a rounded tile tinted with its provider's colour and its provider's mark
 * (Claude, Codex, Practice; anything else gets a neutral one).
 */
export function VendorMark({ v, size = 'sm' }: { v: Vendor; size?: 'xs' | 'sm' | 'md' | 'lg' }) {
  const px = { xs: 16, sm: 20, md: 26, lg: 32 }[size];
  const icon: IconName = VENDOR_ICON[v.cls] ?? (v.name === 'unknown' ? 'unknown' : 'bot');
  return (
    <span className={`vendor avatar avatar-${size} ${v.cls}`} aria-hidden="true" title={v.name} style={{ width: px, height: px }}>
      {/* The Claude Code guy is pixel art: drawn larger so its eyes survive at tile size. */}
      <Icon name={icon} size={Math.round(px * (icon === 'claude' ? 0.85 : 0.62))} strokeWidth={2.2} />
    </span>
  );
}

/**
 * One tooltip for the whole app. Any element with `data-tip` (and optionally `data-kbd`) shows
 * it after a short hover, or at once on keyboard focus. Purely visual: names stay on the
 * controls themselves. Drawn fixed, so panels that clip their content never cut it off.
 */
export function TooltipLayer() {
  const [tip, setTip] = useState<{ text: string; kbd?: string; x: number; y: number; below: boolean } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const current = useRef<Element | null>(null);

  useEffect(() => {
    const clear = () => {
      if (timer.current) clearTimeout(timer.current);
      timer.current = null;
      current.current = null;
      setTip(null);
    };
    const show = (el: HTMLElement) => {
      const text = (el.dataset.tip || el.getAttribute('aria-label') || el.textContent || '').trim();
      const kbd = el.dataset.kbd || undefined;
      if (!text && !kbd) return;
      const r = el.getBoundingClientRect();
      const below = r.top < 64 || r.bottom + 44 < window.innerHeight;
      // Keep the bubble on screen near the edges (it is centred on x).
      const x = Math.min(window.innerWidth - 120, Math.max(120, r.left + r.width / 2));
      setTip({ text, kbd, x, y: below ? r.bottom + 6 : r.top - 6, below });
    };
    const target = (e: Event) => (e.target instanceof Element ? (e.target.closest('[data-tip]') as HTMLElement | null) : null);
    const over = (e: PointerEvent) => {
      if (e.pointerType === 'touch') return;
      const el = target(e);
      if (el === current.current) return;
      clear();
      if (!el) return;
      current.current = el;
      timer.current = setTimeout(() => show(el), 450);
    };
    const focus = (e: FocusEvent) => {
      const el = target(e);
      if (!el || !el.matches(':focus-visible')) return;
      clear();
      current.current = el;
      show(el);
    };
    const key = (e: KeyboardEvent) => e.key === 'Escape' && clear();
    document.addEventListener('pointerover', over);
    document.addEventListener('focusin', focus);
    document.addEventListener('focusout', clear);
    document.addEventListener('pointerdown', clear, true);
    document.addEventListener('keydown', key, true);
    window.addEventListener('scroll', clear, true);
    window.addEventListener('blur', clear);
    return () => {
      clear();
      document.removeEventListener('pointerover', over);
      document.removeEventListener('focusin', focus);
      document.removeEventListener('focusout', clear);
      document.removeEventListener('pointerdown', clear, true);
      document.removeEventListener('keydown', key, true);
      window.removeEventListener('scroll', clear, true);
      window.removeEventListener('blur', clear);
    };
  }, []);

  if (!tip) return null;
  return (
    <div className={`tooltip${tip.below ? ' below' : ' above'}`} style={{ left: tip.x, top: tip.y }} aria-hidden="true">
      {tip.text && <span>{tip.text}</span>}
      {tip.kbd && <kbd className="kbd">{tip.kbd}</kbd>}
    </div>
  );
}
