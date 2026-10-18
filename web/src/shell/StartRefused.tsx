import { useEffect, useRef, type ReactNode } from 'react';
import type { DraftIssue } from '../generated/DraftIssue';
import { Btn } from './ui';
import './start-refused.css';

export type Refusal = { text: string; code?: string; issues?: DraftIssue[] };

/**
 * Why a team could not start, in a dialog over everything, so it is seen on every page and every
 * screen size. A plan hold can be overridden: Start anyway starts the team past it.
 */
export function StartRefused({ refusal, onClose, onAnyway, children }: { refusal: Refusal | null; onClose: () => void; onAnyway?: () => void; children?: ReactNode }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = dialog.current;
    // Older browsers (and test DOMs) without showModal still get it open, inline.
    if (refusal && d && !d.open) { if (typeof d.showModal === 'function') d.showModal(); else d.setAttribute('open', ''); }
    if (!refusal && d?.open) { if (typeof d.close === 'function') d.close(); else d.removeAttribute('open'); }
  }, [refusal]);
  if (!refusal) return null;
  const held = refusal.code === 'plan_limit' && !!onAnyway;
  return (
    <dialog ref={dialog} className="start-refused" role="alertdialog" aria-labelledby="start-refused-title" aria-describedby="start-refused-text"
      onCancel={(e) => { e.preventDefault(); onClose(); }}>
      <h2 id="start-refused-title">{held ? 'Your plan is near its limit' : 'The team could not start'}</h2>
      <p id="start-refused-text">{refusal.text}</p>
      {!!refusal.issues?.length && (
        <ul>{refusal.issues.map((i, n) => <li key={`${i.code}-${n}`}><span aria-hidden="true">{i.severity === 'error' ? '▲' : '△'} </span>{i.message}</li>)}</ul>
      )}
      {children}
      {held && <p className="muted">Starting anyway uses the plan past Sculptured's hold, and paid credits where the plan has them.</p>}
      <div className="start-refused-actions">
        <Btn onClick={onClose}>{held ? 'Not now' : 'OK'}</Btn>
        {held && <Btn variant="primary" onClick={() => { onClose(); onAnyway(); }}>Start anyway</Btn>}
      </div>
    </dialog>
  );
}
