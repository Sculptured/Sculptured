import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Navigate, useLocation, useNavigate } from 'react-router-dom';
import { api, type SetupProgress } from '../api';
import { useHosted } from '../hosted/HostedGate';
import { listConnections } from '../hosted/connections';
import { Tour } from './Tour';

export type StepId = 'welcome' | 'computer' | 'models' | 'computers' | 'tour';
export type StepState = 'done' | 'skipped' | 'todo';
/** How often the setup page reads computers and sign-ins again (tests shorten it). */
export const SETUP_POLL = { ms: 5000 };

type Host = Awaited<ReturnType<typeof api.hosts>>[number];
/** What is true right now, read from the server. null: not known (a read failed or has not finished). */
export type Facts = { computer: boolean | null; cli: boolean | null; connection: boolean | null; hosts: Host[] };
const UNKNOWN: Facts = { computer: null, cli: null, connection: null, hosts: [] };
const EMPTY: SetupProgress = { done: [], skipped: [] };

export type Setup = {
  mode: 'hosted' | 'local';
  steps: StepId[];
  ready: boolean;
  progress: SetupProgress;
  facts: Facts;
  stateOf: (s: StepId) => StepState;
  /** Computer and Models are done (hosted), so the app is open. */
  requiredDone: boolean;
  mark: (s: StepId, how: 'done' | 'skipped') => Promise<void>;
  refresh: () => Promise<void>;
  startTour: () => void;
};

const Ctx = createContext<Setup | null>(null);
/** Setup's progress and live facts; null outside the app (pages rendered on their own). */
export const useSetup = () => useContext(Ctx);

const HOSTED_STEPS: StepId[] = ['welcome', 'computer', 'models', 'computers', 'tour'];
// The desktop app runs agents on the computer it runs on, and clusters are a hosted feature.
const LOCAL_STEPS: StepId[] = ['welcome', 'models', 'tour'];
export const OPTIONAL: StepId[] = ['computers', 'tour'];
const REQUIRED: StepId[] = ['computer', 'models'];

// Open while setup is unfinished: Account (and its Sign out), docs, Community, approving a computer's
// code, the Computers page, and admin pages.
const OPEN = [/^\/setup$/, /^\/account$/, /^\/community(\/|$)/, /^\/what-we-record$/, /^\/connect$/, /^\/computers$/, /^\/admin$/, /^\/ops$/, /^\/docs(\/|$)/];

/** Reads whether a computer is connected, a CLI is signed in on one, and a model connection works. */
async function readFacts(hosted: boolean): Promise<Facts> {
  const [hosts, conns] = await Promise.all([
    hosted ? Promise.resolve().then(() => api.hosts()).catch(() => null) : Promise.resolve(null),
    hosted ? listConnections().catch(() => null) : Promise.resolve([]),
  ]);
  const list = Array.isArray(hosts) ? hosts : [];
  const connection = conns ? conns.some((c) => c.checked?.ok === true) : null;
  const ids: (string | undefined)[] = hosted ? list.map((h) => h.id) : [undefined];
  const clis = await Promise.all(ids.map((id) => Promise.resolve().then(() => api.clis(id)).catch(() => null)));
  const signedIn = clis.some((l) => Array.isArray(l) && l.some((c) => c.signed_in === true));
  const cli = signedIn ? true : ids.length > 0 && clis.every((l) => l === null) ? null : false;
  return { computer: hosted ? (Array.isArray(hosts) ? list.length > 0 : null) : true, cli, connection, hosts: list };
}

const factDone = (f: Facts, s: StepId) => (s === 'computer' ? f.computer === true : s === 'models' ? f.cli === true || f.connection === true : false);

/** Holds setup progress for the app and, hosted, leads app pages to /setup until Computer and Models are done. */
export function SetupGate({ children }: { children: ReactNode }) {
  const session = useHosted();
  const mode = session.kind === 'signed-in' ? 'hosted' : session.kind === 'local' ? 'local' : null;
  if (!mode) return <>{children}</>;
  return <SetupHost mode={mode}>{children}</SetupHost>;
}

/** Someone who already has what a step sets up never has to do it again: the steps the facts show done, added (null: none). */
function withFacts(p: SetupProgress, f: Facts, hosted: boolean): SetupProgress | null {
  const required = hosted ? REQUIRED : ['models' as StepId];
  const auto = required.filter((s) => factDone(f, s) && !p.done.includes(s));
  if (required.every((s) => factDone(f, s) || p.done.includes(s)) && !p.done.includes('welcome')) auto.unshift('welcome');
  return auto.length > 0 ? { done: [...p.done, ...auto], skipped: p.skipped.filter((s) => !auto.includes(s as StepId)) } : null;
}

/** Saved progress and live facts: read once (facts only when the saved progress does not settle the gate), then on `refresh`. */
function useSetupState(hosted: boolean) {
  const [progress, setProgress] = useState<SetupProgress | null>(null);
  const [facts, setFacts] = useState<Facts>(UNKNOWN);
  const [factsRead, setFactsRead] = useState(false);
  const current = useRef<SetupProgress | null>(null);
  const save = useCallback((p: SetupProgress) => {
    current.current = p;
    setProgress(p);
    // Progress also follows from what is connected, so a save that fails costs at most a repeated step.
    return Promise.resolve().then(() => api.saveSetup(p)).then(() => {}, () => {});
  }, []);
  useEffect(() => {
    let live = true;
    void (async () => {
      const saved = await Promise.resolve().then(() => api.setup()).catch(() => null);
      const p: SetupProgress = saved && Array.isArray(saved.done) ? { done: saved.done, skipped: saved.skipped ?? [] } : EMPTY;
      if (!live) return;
      current.current = p;
      setProgress(p);
      if (hosted && REQUIRED.every((s) => p.done.includes(s))) { setFactsRead(true); return; }
      const f = await readFacts(hosted);
      if (!live) return;
      setFacts(f);
      const more = withFacts(p, f, hosted);
      if (more) void save(more);
      setFactsRead(true);
    })();
    return () => { live = false; };
  }, [hosted, save]);
  const refresh = useCallback(async () => { setFacts(await readFacts(hosted)); }, [hosted]);
  const mark = useCallback(async (s: StepId, how: 'done' | 'skipped') => {
    const p = current.current ?? EMPTY;
    const next: SetupProgress = { done: p.done.filter((x) => x !== s), skipped: p.skipped.filter((x) => x !== s) };
    next[how === 'done' ? 'done' : 'skipped'].push(s);
    await save(next);
  }, [save]);
  return { progress, facts, factsRead, refresh, mark };
}

function SetupHost({ mode, children }: { mode: 'hosted' | 'local'; children: ReactNode }) {
  const hosted = mode === 'hosted';
  const { progress, facts, factsRead, refresh, mark } = useSetupState(hosted);
  const [touring, setTouring] = useState(false);
  const loc = useLocation();
  const navigate = useNavigate();

  const value = useMemo<Setup>(() => {
    const p = progress ?? EMPTY;
    const stateOf = (s: StepId): StepState => (p.done.includes(s) || factDone(facts, s) ? 'done' : p.skipped.includes(s) ? 'skipped' : 'todo');
    return {
      mode, steps: hosted ? HOSTED_STEPS : LOCAL_STEPS, ready: progress !== null && factsRead, progress: p, facts, stateOf,
      requiredDone: !hosted || REQUIRED.every((s) => stateOf(s) === 'done'),
      mark, refresh, startTour: () => setTouring(true),
    };
  }, [mode, hosted, progress, factsRead, facts, mark, refresh]);

  // Hosted: wait for the answer before showing an app page, so nobody sees it flash and vanish.
  if (hosted && !value.ready && !OPEN.some((r) => r.test(loc.pathname))) return null;
  if (hosted && value.ready && !value.requiredDone && !OPEN.some((r) => r.test(loc.pathname))) return <Navigate replace to="/setup" />;
  return (
    <Ctx.Provider value={value}>
      {children}
      {touring && <Tour onEnd={(finished) => { setTouring(false); void mark('tour', 'done'); if (finished) navigate('/'); }} />}
    </Ctx.Provider>
  );
}
