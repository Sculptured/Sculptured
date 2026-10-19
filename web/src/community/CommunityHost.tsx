import type { OpenStream } from '../useRigStream';
import { useHosted } from '../hosted/HostedGate';
import { Shell } from '../shell/Shell';
import { CommunityFrame } from './CommunityFrame';

/** Community inside the app when signed in (or on your own computer); in a light frame when signed out. */
export function CommunityHost({ openStream }: { openStream?: OpenStream }) {
  return useHosted().kind === 'signed-out' ? <CommunityFrame /> : <Shell openStream={openStream} />;
}
