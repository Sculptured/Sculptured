import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { boardPath } from '../paths';
import { WORDS } from '../vocab';
import { Panel } from './ui';

/** How long "Connecting…" may show before the page says the live updates are not arriving. */
export const STALL_MS = 8000;

/** The team's live view before its first update: "Connecting…", and after STALL_MS a plain failure with
 * a retry and a way to the Design board, instead of waiting forever. */
export function ConnectingPanel({ rig }: { rig: string }) {
  const [stalled, setStalled] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setStalled(true), STALL_MS);
    return () => clearTimeout(t);
  }, [rig]);
  return (
    <Panel title={<span className="team-title">{rig}</span>} className="connecting">
      {stalled ? (
        <div role="alert" className="pad stall-note">
          <p><strong>Live updates for {rig} aren't arriving.</strong> Sculptured keeps trying in the background. Your agents are not affected.</p>
          <p className="muted">This is usually a dropped network connection, or a network that blocks live connections.</p>
          <p className="stall-actions">
            <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>Try again</button>
            <Link className="btn" to={boardPath(rig, 'design')}>Open the Design board</Link>
          </p>
        </div>
      ) : (
        <p role="status" className="pad muted">{WORDS.connecting(rig)}</p>
      )}
    </Panel>
  );
}
