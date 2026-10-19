import { useCallback, useEffect, useMemo, useState } from 'react';
import { Outlet } from 'react-router-dom';
import type { MeUser } from '../generated/community/MeUser';
import { communityApi } from './api';
import { CommunityContext, type CommunityState } from './context';
import { CommunityTabs } from './CommunityTabs';
import './community.css';

/** Every hosted Community page: your profile (when signed in), the tab row, then the page. */
export function CommunityLayout({ signedIn }: { signedIn: boolean }) {
  const [me, setMe] = useState<MeUser | null>(null);
  const reloadMe = useCallback(async () => {
    // A profile that cannot be read leaves you browsing as if signed out; pages say why when it matters.
    try { setMe((await communityApi.me()).user); } catch { setMe(null); }
  }, []);
  useEffect(() => { if (signedIn) void reloadMe(); }, [signedIn, reloadMe]);
  const state = useMemo<CommunityState>(() => ({ signedIn, me, reloadMe }), [signedIn, me, reloadMe]);
  return (
    <CommunityContext.Provider value={state}>
      <main className="page page-wide community" data-vocab="community">
        <CommunityTabs />
        <Outlet />
      </main>
    </CommunityContext.Provider>
  );
}
