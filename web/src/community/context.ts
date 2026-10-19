import { createContext, useContext } from 'react';
import type { MeUser } from '../generated/community/MeUser';

/** Whether you are signed in, and your Community profile once loaded. */
export type CommunityState = { signedIn: boolean; me: MeUser | null; reloadMe: () => Promise<void> };

export const CommunityContext = createContext<CommunityState | null>(null);

export function useCommunity(): CommunityState {
  const s = useContext(CommunityContext);
  if (!s) throw new Error('useCommunity outside the Community pages');
  return s;
}
