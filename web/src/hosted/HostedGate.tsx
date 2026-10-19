import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { Navigate, useLocation, useNavigate } from 'react-router-dom';
import { loadSession, type HostedSession } from './session';
import { routeTemplate, startAnalytics } from './analytics';
import { remember, takeRemembered } from './afterLogin';

const Ctx = createContext<HostedSession>({ kind: 'local' });
export const useHosted = () => useContext(Ctx);
/** Supplies a session directly (tests, and pages rendered outside the gate). */
export const HostedProvider = Ctx.Provider;
// Community is readable signed out; signing in is only needed to save, publish or edit a profile.
const OPEN = [/^\/join\//, /^\/reset\//, /^\/login$/, /^\/what-we-record$/, /^\/community(\/|$)/];

export function HostedGate({ children }: { children: ReactNode }) {
  const [s, setS] = useState<HostedSession | null>(null);
  const [down, setDown] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const loc = useLocation();
  const navigate = useNavigate();
  useEffect(() => { loadSession().then((x) => { setDown(false); setS(x); }, () => setDown(true)); }, [attempt]);
  // Hosted beta analytics, while signed in (local mode never starts it). See /what-we-record.
  const csrf = s?.kind === 'signed-in' ? s.csrf : null;
  useEffect(() => (csrf ? startAnalytics(csrf, () => routeTemplate(window.location.pathname)) : undefined), [csrf]);
  // Signed in and on the home page: go back to the page that sent this visitor to sign in, if any.
  const signedIn = s?.kind === 'signed-in';
  useEffect(() => {
    if (!signedIn || loc.pathname !== '/') return;
    const back = takeRemembered();
    if (back) navigate(back, { replace: true });
  }, [signedIn, loc.pathname, navigate]);
  if (down) return (
    <main className="hosted-page" role="alert">
      <h1>Sculptured is not answering</h1>
      <p>This is usually brief. Your work is safe.</p>
      <button className="btn btn-primary" onClick={() => setAttempt((n) => n + 1)}>Try again</button>
    </main>
  );
  if (!s) return null;
  if (s.kind === 'signed-out' && !OPEN.some((r) => r.test(loc.pathname))) {
    remember(loc.pathname + loc.search);
    return <Navigate replace to="/login" />;
  }

  return <Ctx.Provider value={s}>{children}</Ctx.Provider>;
}
