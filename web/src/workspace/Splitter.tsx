import { trackPointer } from '../dom';

/** How far one arrow key press moves a splitter, as a fraction of its split. */
export const KEY_STEP = 0.05;

type Props = {
  dir: 'row' | 'col';
  /** The share of the split held by the pane before this splitter, 0 to 1. */
  size: number;
  label: string;
  /** Reads the share as it stands now, which can be ahead of `size` mid-drag. */
  current(): number;
  onResize(delta: number): void;
};

const KEYS = { row: { ArrowLeft: -KEY_STEP, ArrowRight: KEY_STEP }, col: { ArrowUp: -KEY_STEP, ArrowDown: KEY_STEP } } as const;

/** Pointer drag: the share follows the pointer from where it started, as a fraction of the split's span. */
function pointerResize({ dir, current, onResize }: Pick<Props, 'dir' | 'current' | 'onResize'>) {
  return (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const box = e.currentTarget.parentElement?.getBoundingClientRect();
    const span = (dir === 'row' ? box?.width : box?.height) ?? 0;
    if (span <= 0) return;
    const start = current();
    e.currentTarget.focus();
    trackPointer(e, (dx, dy) => onResize(start + (dir === 'row' ? dx : dy) / span - current()), () => {});
  };
}

/** The gap between two panes of a split: drag it, or focus it and use the arrow keys. */
export function Splitter({ dir, size, label, current, onResize }: Props) {
  const onKeyDown = (e: React.KeyboardEvent) => {
    const step = (KEYS[dir] as Record<string, number>)[e.key];
    if (step === undefined) return;
    e.preventDefault();
    onResize(step);
  };
  return (
    <div
      className={`ws-splitter ws-splitter-${dir}`}
      role="separator"
      aria-orientation={dir === 'row' ? 'vertical' : 'horizontal'}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(size * 100)}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onPointerDown={pointerResize({ dir, current, onResize })}
    />
  );
}
