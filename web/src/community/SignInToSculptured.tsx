import { useEffect, useRef, useState } from 'react';
import type { CommunityPairStarted } from '../generated/CommunityPairStarted';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { linkApi, useCommunityLink } from './link';
import { LoadProblem } from './LoadProblem';
import './community.css';

const W = COMMUNITY_WORDS.signIn;
type Phase = { kind: 'starting' } | { kind: 'code'; started: CommunityPairStarted } | { kind: 'ended'; status: 'expired' | 'denied' } | { kind: 'done' } | { kind: 'failed'; error: unknown };

/** Signing in from this computer: a code to approve on the Sculptured sign-in page, then who you are. */
export function SignInToSculptured({ onSignedIn }: { onSignedIn?: () => void }) {
  const { link, refresh } = useCommunityLink();
  const [phase, setPhase] = useState<Phase>({ kind: 'starting' });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    setPhase({ kind: 'starting' });
    linkApi.pair().then((started) => { if (live) setPhase({ kind: 'code', started }); }, (error) => { if (live) setPhase({ kind: 'failed', error }); });
    return () => { live = false; };
  }, [attempt]);
  usePairPoll(phase.kind === 'code', setPhase, refresh, onSignedIn);
  const again = () => setAttempt((n) => n + 1);
  return (
    <section className="cm-signin-code" aria-label={W.title}>
      <h2>{W.title}</h2>
      {phase.kind === 'starting' && <p role="status" className="muted">Getting a code…</p>}
      {phase.kind === 'failed' && <LoadProblem error={phase.error} onRetry={again} />}
      {phase.kind === 'code' && <CodePanel started={phase.started} origin={link?.origin ?? null} />}
      {phase.kind === 'ended' && <PairOutcome status={phase.status} onAgain={again} />}
      {phase.kind === 'done' && <p role="status">{link?.account ? W.done(link.account.name) : 'Signed in.'}</p>}
    </section>
  );
}

/** Asks every 2 s while the code waits for approval; stops when it is answered or the panel closes. */
function usePairPoll(waiting: boolean, setPhase: (p: Phase) => void, refresh: () => Promise<void>, onSignedIn?: () => void) {
  const busy = useRef(false);
  const signedIn = useRef(onSignedIn);
  signedIn.current = onSignedIn;
  useEffect(() => {
    if (!waiting) return;
    let live = true;
    const tick = async () => {
      if (busy.current) return;
      busy.current = true;
      try {
        const { status } = await linkApi.pairStatus();
        if (!live || status === 'pending') return;
        if (status === 'approved') {
          await refresh();
          if (live) { setPhase({ kind: 'done' }); signedIn.current?.(); }
        } else setPhase({ kind: 'ended', status });
      } catch {
        // Not answered this time: ask again on the next tick.
      } finally {
        busy.current = false;
      }
    };
    const t = setInterval(() => void tick(), 2000);
    return () => { live = false; clearInterval(t); };
  }, [waiting, setPhase, refresh]);
}

/** The code, large, and the sign-in page's link only when it is on the Sculptured server Community uses. */
function CodePanel({ started, origin }: { started: CommunityPairStarted; origin: string | null }) {
  const ours = origin !== null && started.verify_url.startsWith(`${origin}/`);
  return (
    <>
      <p>{W.enter}</p>
      <p className="cm-code" aria-live="polite">{started.user_code}</p>
      {ours && <p><a href={started.verify_url} target="_blank" rel="noopener noreferrer">{W.open}</a></p>}
      <p role="status" className="muted">{W.waiting}</p>
    </>
  );
}

function PairOutcome({ status, onAgain }: { status: 'expired' | 'denied'; onAgain: () => void }) {
  return (
    <div className="cm-problem" role="alert">
      <p>{status === 'expired' ? W.expired : W.denied}</p>
      <Btn size="sm" onClick={onAgain}>{W.again}</Btn>
    </div>
  );
}
