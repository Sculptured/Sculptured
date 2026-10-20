import { useLocation } from 'react-router-dom';
import { remember } from '../hosted/afterLogin';

/** Why signing in is needed here, with the way to do it; afterwards the visitor comes back to this page. */
export function SignInPrompt({ why }: { why: string }) {
  const here = useLocation();
  return <p className="cm-signin">{why} <a href="/login" onClick={() => remember(`${here.pathname}${here.search}`)}>Sign in</a></p>;
}
