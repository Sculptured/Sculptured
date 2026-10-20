import { useSyncExternalStore } from 'react';
import { apiRequest } from '../api';
import { errorText } from '../errors';
import type { CommunityLinkView } from '../generated/CommunityLinkView';
import type { CommunityPairStarted } from '../generated/CommunityPairStarted';
import type { CommunityPairStatus } from '../generated/CommunityPairStatus';
import { useHosted } from '../hosted/HostedGate';
import type { HostedSession } from '../hosted/session';

/** Community for this app: the hosted app signs in to it with the Sculptured account itself. */
export function communityFor(s: HostedSession): { available: boolean; signedIn: boolean } {
  return { available: s.kind !== 'local', signedIn: s.kind === 'signed-in' };
}

/** This computer's link to Community (C5), answered by Sculptured on this computer. */
export const linkApi = {
  get: () => apiRequest<CommunityLinkView>('GET', '/api/community-link'),
  enable: (origin?: string) => apiRequest<CommunityLinkView>('POST', '/api/community-link/enable', origin ? { origin } : {}),
  pair: () => apiRequest<CommunityPairStarted>('POST', '/api/community-link/pair', {}),
  pairStatus: () => apiRequest<CommunityPairStatus>('GET', '/api/community-link/pair'),
  turnOff: () => apiRequest<{ enabled: false }>('DELETE', '/api/community-link'),
  dismissKeyLeft: () => apiRequest<CommunityLinkView>('DELETE', '/api/community-link/key-left'),
};

/** The hosted app's link, from its session (it never asks /api/community-link). Null on this computer. */
export function hostedLink(s: HostedSession): CommunityLinkView | null {
  if (s.kind === 'local') return null;
  const signedIn = s.kind === 'signed-in';
  return { enabled: true, origin: null, signed_in: signedIn, account: signedIn ? { name: s.user.name, handle: null } : null };
}

type Snapshot = { link: CommunityLinkView | null; error: string };
const empty: Snapshot = { link: null, error: '' };
let snapshot: Snapshot = empty;
let started = false;
const listeners = new Set<() => void>();

function publish(next: Snapshot) {
  snapshot = next;
  listeners.forEach((l) => l());
}

/** Reads the link again for every page that shows it (after turning on, signing in, or a refused key). */
export async function refreshLink(): Promise<void> {
  started = true;
  try {
    publish({ link: await linkApi.get(), error: '' });
  } catch (e) {
    publish({ link: snapshot.link, error: errorText(e) });
  }
}

/** A refused key (`device_unknown`): read the link again so pages show signed out. Only once the link is in use (never hosted). */
export function linkKeyRefused() {
  if (started) void refreshLink();
}

function subscribe(l: () => void) {
  listeners.add(l);
  if (!started) void refreshLink();
  return () => { listeners.delete(l); };
}
const noSubscribe = () => () => {};

/** Tests start each case without a link read. */
export function resetCommunityLink() {
  snapshot = empty;
  started = false;
}

/** The link, read once and shared by every page; the hosted app's comes from its session and is never fetched. */
export function useCommunityLink(): { link: CommunityLinkView | null; error: string; refresh: () => Promise<void> } {
  const s = useHosted();
  const local = s.kind === 'local';
  const snap = useSyncExternalStore(local ? subscribe : noSubscribe, () => (local ? snapshot : empty));
  if (!local) return { link: hostedLink(s), error: '', refresh: async () => {} };
  return { ...snap, refresh: refreshLink };
}
