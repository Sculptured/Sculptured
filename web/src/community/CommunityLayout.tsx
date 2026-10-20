import { useCallback, useEffect, useMemo, useState } from 'react';
import { Outlet } from 'react-router-dom';
import type { MeUser } from '../generated/community/MeUser';
import { communityApi } from './api';
import { CommunityContext, type CommunityState } from './context';
import { CommunityTabs } from './CommunityTabs';
import { SignInToSculptured } from './SignInToSculptured';
import './community.css';

type Me = { user: MeUser | null; state: CommunityState['meState']; error: unknown };

/** Every Community page: your profile (when signed in), the tab row, then the page. `signInOpen`
 * (this computer, after "Turn on and sign in") shows the code to approve above the page, and then
 * who you are signed in as. */
export function CommunityLayout({ signedIn, workspace, signInOpen = false }: { signedIn: boolean; workspace: boolean; signInOpen?: boolean }) {
  const [me, setMe] = useState<Me>({ user: null, state: signedIn ? 'loading' : 'ready', error: null });
  const reloadMe = useCallback(async () => {
    // Browsing keeps working when your profile cannot be read; pages that need it say why (MeGate).
    try { setMe({ user: (await communityApi.me()).user, state: 'ready', error: null }); } catch (e) { setMe({ user: null, state: 'failed', error: e }); }
  }, []);
  // Signed out (or the key was refused): no profile, so pages ask to sign in again.
  useEffect(() => { if (signedIn) void reloadMe(); else setMe({ user: null, state: 'ready', error: null }); }, [signedIn, reloadMe]);
  const state = useMemo<CommunityState>(() => ({ signedIn, workspace, me: me.user, meState: me.state, meError: me.error, reloadMe }), [signedIn, workspace, me, reloadMe]);
  return (
    <CommunityContext.Provider value={state}>
      <main className="page page-wide community" data-vocab="community">
        <CommunityTabs />
        {signInOpen && <SignInToSculptured />}
        <Outlet />
      </main>
    </CommunityContext.Provider>
  );
}
