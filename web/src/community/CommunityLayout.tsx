import { useCallback, useEffect, useMemo, useState } from 'react';
import { Outlet } from 'react-router-dom';
import type { MeUser } from '../generated/community/MeUser';
import { communityApi } from './api';
import { CommunityContext, type CommunityState } from './context';
import { CommunityTabs } from './CommunityTabs';
import './community.css';

/** Every hosted Community page: your profile (when signed in), the tab row, then the page. */
export function CommunityLayout({ signedIn }: { signedIn: boolean }) {
  const [me, setMe] = useState<{ user: MeUser | null; state: CommunityState['meState']; error: unknown }>({ user: null, state: signedIn ? 'loading' : 'ready', error: null });
  const reloadMe = useCallback(async () => {
    // Browsing keeps working when your profile cannot be read; pages that need it say why (MeGate).
    try { setMe({ user: (await communityApi.me()).user, state: 'ready', error: null }); } catch (e) { setMe({ user: null, state: 'failed', error: e }); }
  }, []);
  useEffect(() => { if (signedIn) void reloadMe(); }, [signedIn, reloadMe]);
  const state = useMemo<CommunityState>(() => ({ signedIn, me: me.user, meState: me.state, meError: me.error, reloadMe }), [signedIn, me, reloadMe]);
  return (
    <CommunityContext.Provider value={state}>
      <main className="page page-wide community" data-vocab="community">
        <CommunityTabs />
        <Outlet />
      </main>
    </CommunityContext.Provider>
  );
}
