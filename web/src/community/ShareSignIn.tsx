import { useState } from 'react';
import { useHosted } from '../hosted/HostedGate';
import { Btn } from '../shell/ui';
import { useCommunityLink } from './link';
import { SignInToSculptured } from './SignInToSculptured';

/** Signing in to share a component to Community, from the board: on this computer the Turn on
 * page while Community is off, else the code to approve right here; hosted, the sign-in page.
 * Plain links, so it works wherever the board is drawn. */
export function ShareSignIn() {
  const hosted = useHosted().kind !== 'local';
  const { link } = useCommunityLink();
  const [open, setOpen] = useState(false);
  if (hosted) return <a className="btn btn-sm" href="/login">Sign in to share</a>;
  if (!link) return null;
  if (!link.enabled) return <a className="btn btn-sm" href="/community">Turn on Community to share</a>;
  if (open) return <SignInToSculptured />;
  return <Btn size="sm" onClick={() => setOpen(true)}>Sign in to share</Btn>;
}
