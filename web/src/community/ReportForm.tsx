import { useState } from 'react';
import { errorText } from '../errors';
import { Btn } from '../shell/ui';
import { useCommunity } from './context';

/** "Report this …": a reason (5–1000 characters) sent to moderators. Signed out, nothing shows. */
export function ReportForm({ what, send }: { what: string; send: (reason: string) => Promise<unknown> }) {
  const { signedIn } = useCommunity();
  const [state, setState] = useState({ sent: false, error: '' });
  if (!signedIn) return null;
  const go = async (reason: string) => {
    try { await send(reason); setState({ sent: true, error: '' }); } catch (e) { setState({ sent: false, error: errorText(e) }); }
  };
  return (
    <details className="cm-report">
      <summary>Report this {what}</summary>
      {state.sent ? <p>Thanks. A moderator will look at it.</p> : (
        <form onSubmit={(e) => { e.preventDefault(); void go(new FormData(e.currentTarget).get('reason')?.toString() ?? ''); }}>
          <label>What is wrong with it?<textarea name="reason" required minLength={5} maxLength={1000} /></label>
          <Btn type="submit">Send report</Btn>
          {state.error && <p className="tone-red" role="alert">{state.error}</p>}
        </form>
      )}
    </details>
  );
}
