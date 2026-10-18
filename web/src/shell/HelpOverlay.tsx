import { useEffect, useRef, useState } from 'react';
import { KEYMAP, WHERE_LABEL } from '../keys';
import { Btn, Icon } from './ui';

/** What the two kinds of board line mean. The board shows it only while a dashed line is drawn; `?` always has it. */
export const LEGEND = [
  { dashed: false, words: 'hands work to (it starts after)' },
  { dashed: true, words: 'reports to / watches (not enforced)' },
] as const;

export function ConnectionLegend({ className }: { className?: string }) {
  return (
    <p className={className}>
      {LEGEND.map((l) => (
        <span key={l.words}>
          <svg aria-hidden="true" width="20" height="6"><line x1="0" y1="3" x2="20" y2="3" stroke="currentColor" strokeWidth="1.5" strokeDasharray={l.dashed ? '4 3' : undefined} /></svg> {l.words}
        </span>
      ))}
    </p>
  );
}

/** `?`: the full keymap with a filter, and what the board's lines mean. */
export function HelpOverlay({ onClose, onCommand }: { onClose: () => void; /** Opens the command palette. */ onCommand?: () => void }) {
  const [q, setQ] = useState('');
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    input.current?.focus();
    return () => {
      if (prev && document.contains(prev)) prev.focus?.();
    };
  }, []);
  const rows = KEYMAP.filter((k) => `${k.keys} ${k.what} ${k.where}`.toLowerCase().includes(q.toLowerCase().trim()));
  const wheres = [...new Set(rows.map((r) => r.where))];
  const legend = !q.trim() || /line|arrow|hand|report|watch|legend|connect/i.test(q);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="help panel"
        role="dialog"
        aria-modal="true"
        aria-label="Keys"
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          }
        }}
      >
        <div className="panel-bar">
          <span className="panel-title">
            <Icon name="keyboard" size={16} /> Keyboard shortcuts
          </span>
          <span className="panel-rule" aria-hidden="true" />
          <Btn variant="ghost" icon="x" iconOnly onClick={onClose} aria-label="Close" kbd="esc" />
        </div>
        <div className="palette-input">
          <span className="prompt" aria-hidden="true">
            <Icon name="search" size={16} />
          </span>
          <input ref={input} aria-label="Filter keys" placeholder="filter keys" value={q} onChange={(e) => setQ(e.target.value)} spellCheck={false} />
        </div>
        {onCommand && (
          <p className="help-palette">
            <kbd>Ctrl+K</kbd>
            <span>finds any agent and runs any command, each listed with its key.</span>
            <Btn size="sm" icon="search" onClick={onCommand}>
              Open it
            </Btn>
          </p>
        )}
        <div className="help-body">
          {rows.length === 0 && !legend && <p className="muted pad">No keys match.</p>}
          {wheres.map((w) => (
            <section key={w} aria-label={w} className="help-sec">
              <h3>{WHERE_LABEL[w]}</h3>
              <dl>
                {rows
                  .filter((r) => r.where === w)
                  .map((r) => (
                    <div key={`${r.keys}-${r.what}`} className="help-row">
                      <dt>
                        <kbd>{r.keys}</kbd>
                      </dt>
                      <dd>{r.what}</dd>
                    </div>
                  ))}
              </dl>
            </section>
          ))}
          {legend && (
            <section aria-label="Lines on the board" className="help-sec">
              <h3>Lines on the board</h3>
              <ConnectionLegend className="help-legend muted" />
            </section>
          )}
        </div>
      </div>
    </div>
  );
}
