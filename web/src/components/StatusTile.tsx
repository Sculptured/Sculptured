import type { ReactNode } from 'react';
import './statusTile.css';

/**
 * How a reading stands: its colour bar (and its glyph's hue) only repeat what the glyph and words say.
 * `verified` is a check Sculptured ran itself (DESIGN.md's green rule).
 */
export type TileTone = 'ok' | 'verified' | 'warn' | 'bad' | 'idle' | 'unknown' | 'plain';

type Props = { label: string; glyph?: string; tone?: TileTone; className?: string; children: ReactNode };

/** One reading of a status strip (Control's header, Arena's): a small caps label, then glyph and value. */
export function StatusTile({ label, glyph, tone = 'plain', className, children }: Props) {
  return (
    <li className={`status-tile status-tile-${tone}${className ? ` ${className}` : ''}`}>
      <span className="status-tile-label">{label}</span>
      <span className="status-tile-value">
        {glyph !== undefined && <span className="status-tile-glyph" aria-hidden="true">{glyph}</span>}
        {glyph !== undefined && ' '}
        {children}
      </span>
    </li>
  );
}
