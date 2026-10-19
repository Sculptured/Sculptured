import type { ReactNode } from 'react';
import type { MeUser } from '../generated/community/MeUser';
import { useCommunity } from './context';
import { LoadProblem } from './LoadProblem';
import { SignInPrompt } from './SignInPrompt';

/** A page that needs your profile: waits for it, says why it cannot be read, or asks you to sign in. */
export function MeGate({ why, children }: { why: string; children: (me: MeUser) => ReactNode }) {
  const { me, meState, meError, reloadMe } = useCommunity();
  if (meState === 'loading') return <p role="status" className="muted">Loading your profile…</p>;
  if (meState === 'failed') return <LoadProblem error={meError} onRetry={() => void reloadMe()} />;
  if (!me) return <SignInPrompt why={why} />;
  return <>{children(me)}</>;
}
