import { useState } from 'react';

/** A project in the Trash: restore it, or delete it forever after a second, explicit confirmation. */
export function TrashItem({ title, busy, onRestore, onDelete, picked, onPick }: { title: string; busy: boolean; onRestore: () => void; onDelete: () => void; picked?: boolean; onPick?: () => void }) {
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="trash-item">
      {onPick && <input type="checkbox" aria-label={`Select ${title}`} checked={!!picked} onChange={onPick} />}
      <span>{title}</span>
      <button className="btn btn-sm" disabled={busy} onClick={onRestore}>Restore</button>
      <button className="btn btn-sm" disabled={busy} aria-label={`Delete ${title} forever`} onClick={() => setConfirming(true)}>Delete forever</button>
      {confirming && (
        <div role="alert" className="trash-confirm">
          <p>Delete {title} forever? Its design, runs and history can't be restored. Your project folder is not touched.</p>
          <button className="btn btn-sm tone-red" disabled={busy} onClick={() => { setConfirming(false); onDelete(); }}>Yes, delete forever</button>
          <button className="btn btn-sm" onClick={() => setConfirming(false)}>Cancel</button>
        </div>
      )}
    </div>
  );
}
