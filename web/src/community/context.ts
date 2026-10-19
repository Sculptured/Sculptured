import { createContext, useContext } from 'react';
import type { MeUser } from '../generated/community/MeUser';

/** Whether you are signed in, and your Community profile once loaded. */
/** `meState`: whether your profile is still loading, loaded (`me` is null when signed out), or could not be read (`meError`). */
export type CommunityState = { signedIn: boolean; me: MeUser | null; meState: 'loading' | 'ready' | 'failed'; meError: unknown; reloadMe: () => Promise<void> };

export const CommunityContext = createContext<CommunityState | null>(null);

export function useCommunity(): CommunityState {
  const s = useContext(CommunityContext);
  if (!s) throw new Error('useCommunity outside the Community pages');
  return s;
}
