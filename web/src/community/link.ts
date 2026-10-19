import type { HostedSession } from '../hosted/session';

/** Community for this app: the hosted app signs in to it with the Sculptured account itself; the
 * self-run app reaches it later, once it can pair with a Sculptured account. */
export function communityFor(s: HostedSession): { available: boolean; signedIn: boolean } {
  return { available: s.kind !== 'local', signedIn: s.kind === 'signed-in' };
}
