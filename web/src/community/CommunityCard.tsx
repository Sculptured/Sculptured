import { useState } from 'react';
import { Link } from 'react-router-dom';
import { errorText } from '../errors';
import type { CommunityLinkView } from '../generated/CommunityLinkView';
import { Btn } from '../shell/ui';
import { COMMUNITY_WORDS } from '../vocab';
import { linkApi, useCommunityLink } from './link';
import { KeyLeftNote } from './KeyLeftNote';
import { SignInToSculptured } from './SignInToSculptured';

/** The Account page's Community card on this computer: off (and where to turn it on), or on with
 * the server, who you are signed in as, and Turn off. */
export function CommunityCard() {
  const { link, error, refresh } = useCommunityLink();
  if (!link) return error ? <p className="tone-red" role="alert">{error}</p> : <p role="status" className="muted">Loading…</p>;
  return (
    <>
      <KeyLeftNote />
      {link.enabled ? <CommunityOn link={link} refresh={refresh} /> : <p>Community is off on this computer; nothing is sent to Sculptured’s servers. <Link to="/community">Open Community</Link></p>}
    </>
  );
}

function host(origin: string | null): string {
  try { return origin ? new URL(origin).host : ''; } catch { return origin ?? ''; }
}

function CommunityOn({ link, refresh }: { link: CommunityLinkView; refresh: () => Promise<void> }) {
  const [signingIn, setSigningIn] = useState(false);
  const [state, setState] = useState({ busy: false, error: '' });
  const turnOff = async () => {
    setState({ busy: true, error: '' });
    try { await linkApi.turnOff(); await refresh(); } catch (e) { setState({ busy: false, error: errorText(e) }); }
  };
  const a = link.account;
  return (
    <div className="cm-card-on">
      <p>Community is on, through {host(link.origin)}.</p>
      {link.signed_in ? <p>{a ? `Signed in as ${a.name}${a.handle ? ` (@${a.handle})` : ''}` : 'Signed in.'}</p>
        : signingIn ? <SignInToSculptured /> : <Btn onClick={() => setSigningIn(true)}>{COMMUNITY_WORDS.signIn.title}</Btn>}
      <div className="cm-actions"><Btn disabled={state.busy} onClick={() => void turnOff()}>{COMMUNITY_WORDS.turnOff}</Btn></div>
      {state.error && <p className="tone-red" role="alert">{state.error}</p>}
    </div>
  );
}
