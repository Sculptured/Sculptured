import { useState } from 'react';
import { errorText } from '../errors';
import { Btn } from '../shell/ui';
import { linkApi, useCommunityLink } from './link';

function host(origin: string): string {
  try { return new URL(origin).host; } catch { return origin; }
}

/** After a sign-in this computer forgot could not be revoked on the server: where to remove it. */
export function KeyLeftNote() {
  const { link, refresh } = useCommunityLink();
  const [error, setError] = useState('');
  const origin = link?.key_left_on;
  if (!origin) return null;
  const dismiss = async () => {
    try { await linkApi.dismissKeyLeft(); await refresh(); } catch (e) { setError(errorText(e)); }
  };
  return (
    <div className="cm-problem" role="note">
      <p>
        Sculptured could not remove this computer’s Community sign-in from {host(origin)}. It may still work there: remove
        “Community access from …” on that account’s <a href={`${origin}/computers`} target="_blank" rel="noopener noreferrer">Computers page</a>.
      </p>
      <Btn size="sm" onClick={() => void dismiss()}>Dismiss</Btn>
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}
