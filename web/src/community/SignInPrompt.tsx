import { useState } from 'react';
import { useLocation } from 'react-router-dom';
import { remember } from '../hosted/afterLogin';
import { useHosted } from '../hosted/HostedGate';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { SignInToSculptured } from './SignInToSculptured';

/** Why signing in is needed here, with the way to do it: on this computer a code to approve, in the
 * hosted app the sign-in page (and back to this page afterwards). */
export function SignInPrompt({ why }: { why: string }) {
  return useHosted().kind === 'local' ? <LocalSignIn why={why} /> : <HostedSignIn why={why} />;
}

function HostedSignIn({ why }: { why: string }) {
  const here = useLocation();
  return <p className="cm-signin">{why} <a href="/login" onClick={() => remember(`${here.pathname}${here.search}`)}>Sign in</a></p>;
}

function LocalSignIn({ why }: { why: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="cm-signin">
      <p>{why} {!open && <Btn size="sm" onClick={() => setOpen(true)}>{COMMUNITY_WORDS.signIn.title}</Btn>}</p>
      {open && <SignInToSculptured />}
    </div>
  );
}
