import { useCallback, useEffect, useId, useState, type ReactNode } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { api } from '../api';
import { Clusters } from '../hosted/Clusters';
import { ComputerClis, type Computer } from '../hosted/ComputersPage';
import { ConnectComputer } from '../hosted/ConnectComputer';
import { useHosted } from '../hosted/HostedGate';
import { gw } from '../hosted/session';
import { setKindAll } from '../draft/models';
import { boardPath } from '../paths';
import { CliAccounts } from '../shell/CliAccounts';
import { failText, ModelConnections } from './ModelConnections';
import { OPTIONAL, SETUP_POLL, useSetup, type Setup, type StepId, type StepState } from './SetupGate';
import '../hosted/hosted.css';
import { STEP_NAME } from './steps';
import './setup.css';

const STATE_WORDS: Record<StepState, { mark: string; text: string }> = { done: { mark: '✓', text: 'Done' }, skipped: { mark: '–', text: 'Skipped' }, todo: { mark: '○', text: 'To do' } };

/**
 * `/setup`: Welcome → Computer → Models → More computers (optional) → First project (optional), one
 * step at a time. Progress is saved on the server and it resumes at the first step left.
 */
export function SetupPage() {
  const setup = useSetup();
  const [params] = useSearchParams();
  const asked = params.get('step') as StepId | null;
  const [step, setStep] = useState<StepId | 'finished' | null>(null);
  useEffect(() => {
    if (!setup?.ready || step !== null) return;
    if (asked && setup.steps.includes(asked)) { setStep(asked); return; }
    setStep(setup.steps.find((s) => setup.stateOf(s) === 'todo') ?? 'finished');
  }, [setup, step, asked]);
  if (!setup) return null;
  return (
    <main className="setup-page">
      <div className="setup-top">
        <h1>Set up Sculptured</h1>
        <span className="tb-fill" />
        <Link to="/account">Account</Link>
        {setup.requiredDone && <Link to="/">Go to Projects</Link>}
      </div>
      <StepBar setup={setup} current={step} onPick={setStep} />
      {!setup.ready || step === null ? <p className="muted" role="status">Loading…</p> : <StepBody setup={setup} step={step} go={setStep} />}
    </main>
  );
}

function StepBar({ setup, current, onPick }: { setup: Setup; current: StepId | 'finished' | null; onPick: (s: StepId) => void }) {
  return (
    <nav className="setup-steps" aria-label="Setup steps">
      <ol>
        {setup.steps.map((s) => {
          const state = setup.stateOf(s);
          const w = state === 'todo' && OPTIONAL.includes(s) ? { mark: '○', text: 'Optional' } : STATE_WORDS[state];
          return (
            <li key={s}>
              <button type="button" aria-current={current === s ? 'step' : undefined} onClick={() => onPick(s)}>
                <span className="setup-step-name">{STEP_NAME[s]}</span>
                <span className={`setup-step-state ${state}`}><span aria-hidden="true">{w.mark} </span>{w.text}</span>
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

type Go = (s: StepId | 'finished') => void;

function StepBody({ setup, step, go }: { setup: Setup; step: StepId | 'finished'; go: Go }) {
  const navigate = useNavigate();
  if (step === 'finished') return <Finished setup={setup} />;
  const i = setup.steps.indexOf(step);
  const after: StepId | 'finished' = i + 1 < setup.steps.length ? setup.steps[i + 1] : 'finished';
  const nav = {
    back: i > 0 ? () => go(setup.steps[i - 1]) : undefined,
    next: async (how: 'done' | 'skipped') => { await setup.mark(step, how); if (after === 'finished' && setup.requiredDone) navigate('/'); else go(after); },
  };
  switch (step) {
    case 'welcome': return <Welcome setup={setup} nav={nav} />;
    case 'computer': return <ComputerStep setup={setup} nav={nav} />;
    case 'models': return <ModelsStep setup={setup} nav={nav} />;
    case 'computers': return <MoreComputers nav={nav} />;
    case 'tour': return <FirstProject setup={setup} nav={nav} />;
  }
}

type Nav = { back?: () => void; next: (how: 'done' | 'skipped') => Promise<void> };

/** Back, Continue (with why it is off, in words) and, for an optional step, Skip. */
function StepNav({ nav, ready = true, reason, optional, noContinue }: { nav: Nav; ready?: boolean; reason?: string; optional?: boolean; noContinue?: boolean }) {
  const ids = useId();
  return (
    <div className="setup-nav">
      {nav.back && <button type="button" className="btn" onClick={nav.back}>Back</button>}
      <span className="tb-fill" />
      {optional && <button type="button" className="btn btn-ghost" onClick={() => void nav.next('skipped')}>Skip</button>}
      {!noContinue && (
        <button type="button" className="btn btn-primary" disabled={!ready} aria-describedby={!ready && reason ? `${ids}-why` : undefined} onClick={() => void nav.next('done')}>Continue</button>
      )}
      {!ready && reason && <p id={`${ids}-why`} className="reason fld-reason">{reason}</p>}
    </div>
  );
}

function Step({ title, children }: { title: string; children: ReactNode }) {
  const ids = useId();
  return <section className="setup-body" aria-labelledby={`${ids}-h`}><h2 id={`${ids}-h`}>{title}</h2>{children}</section>;
}

function Welcome({ setup, nav }: { setup: Setup; nav: Nav }) {
  return (
    <Step title="Welcome">
      <p>Sculptured runs teams of AI agents, such as Claude Code and Codex, and lets you watch and steer them while they work.</p>
      {setup.mode === 'hosted'
        ? <p>Agents work on your own computers, so first you connect one, then give agents a way to reach a model.</p>
        : <p>Agents work on this computer, so the one thing to set up is a way for them to reach a model.</p>}
      <p>After that you can {setup.mode === 'hosted' ? 'add more computers or a cluster, and ' : ''}try a first project with a short tour.</p>
      <StepNav nav={nav} />
    </Step>
  );
}

/** Reads computers and sign-ins again every few seconds while a step waits for them. */
function useLiveFacts(setup: Setup) {
  const [names, setNames] = useState<Record<string, string>>({});
  const hosted = setup.mode === 'hosted';
  const { refresh } = setup;
  const read = useCallback(() => {
    void refresh();
    if (hosted) gw<Computer[]>('GET', '/devices').then((list) => setNames(Object.fromEntries((Array.isArray(list) ? list : []).map((c) => [c.id, c.label ?? c.name]))), () => {});
  }, [refresh, hosted]);
  useEffect(() => {
    read();
    const t = setInterval(read, SETUP_POLL.ms);
    return () => clearInterval(t);
  }, [read]);
  return (id: string) => names[id] ?? id;
}

function ComputerStep({ setup, nav }: { setup: Setup; nav: Nav }) {
  const nameOf = useLiveFacts(setup);
  const [cluster, setCluster] = useState(false);
  const hosts = setup.facts.hosts;
  const connected = setup.stateOf('computer') === 'done' && hosts.length > 0;
  return (
    <Step title="Connect a computer">
      <p>Agents run on your own computer, in the folders you allow, with its Claude Code and Codex. Copy the command below and run it on that computer: this page updates when it connects.</p>
      {connected ? (
        <ul className="setup-list" aria-label="Connected computers">
          {hosts.map((h) => (
            <li key={h.id}>
              <span className="tone-green"><span aria-hidden="true">✓ </span>{nameOf(h.id)} is connected.</span>
              <ComputerClis id={h.id} name={nameOf(h.id)} canSignIn={false} />
            </li>
          ))}
        </ul>
      ) : (
        <p role="status" className="setup-live"><span aria-hidden="true">○</span> Waiting for a computer to connect…</p>
      )}
      <ConnectComputer />
      <p><button type="button" className="btn" aria-expanded={cluster} onClick={() => setCluster(!cluster)}>My computer is a cluster</button></p>
      {cluster && <><p className="muted">A cluster&apos;s login node counts as your computer once its runner connects.</p><Clusters /></>}
      <StepNav nav={nav} ready={setup.stateOf('computer') === 'done'} reason="Continue opens once a computer is connected." />
    </Step>
  );
}

function ModelsStep({ setup, nav }: { setup: Setup; nav: Nav }) {
  const nameOf = useLiveFacts(setup);
  const hosted = setup.mode === 'hosted';
  const [working, setWorking] = useState(false);
  const ready = setup.stateOf('models') === 'done' || working;
  return (
    <Step title="Connect a model">
      <p>Agents need a way to reach a model: a Claude or ChatGPT subscription signed in on your computer, or a pay-per-use key. At least one has to work.</p>
      <h3>Subscriptions</h3>
      {hosted ? (
        setup.facts.hosts.length === 0 ? <p className="muted">Connect a computer first: subscriptions are signed in on it.</p> : (
          <ul className="setup-list" aria-label="Sign-ins on your computers">
            {setup.facts.hosts.map((h) => (
              <li key={h.id}><strong>{nameOf(h.id)}</strong><ComputerClis id={h.id} name={nameOf(h.id)} canSignIn={h.policy?.capabilities.includes('cli_sign_in') ?? false} /></li>
            ))}
          </ul>
        )
      ) : <CliAccounts />}
      {hosted && <ModelConnections onWorking={(ok) => { setWorking(ok); void setup.refresh(); }} />}
      <StepNav nav={nav} ready={ready} reason="Continue opens once Claude Code or Codex is signed in, or a key passes its check." />
    </Step>
  );
}

function MoreComputers({ nav }: { nav: Nav }) {
  const [open, setOpen] = useState<'computer' | 'cluster' | null>(null);
  return (
    <Step title="More computers">
      <p>Agents on different computers work as one team: they coordinate by messages, and each computer&apos;s files and data stay where they are. A cluster&apos;s login node can be one of them.</p>
      <p className="setup-live">
        <button type="button" className="btn" aria-expanded={open === 'computer'} onClick={() => setOpen(open === 'computer' ? null : 'computer')}>Connect another computer</button>
        <button type="button" className="btn" aria-expanded={open === 'cluster'} onClick={() => setOpen(open === 'cluster' ? null : 'cluster')}>Add a cluster</button>
      </p>
      {open === 'computer' && <ConnectComputer />}
      {open === 'cluster' && <Clusters />}
      <StepNav nav={nav} optional />
    </Step>
  );
}

function FirstProject({ setup, nav }: { setup: Setup; nav: Nav }) {
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const create = async () => {
    setBusy(true); setError('');
    try {
      const rec = await api.createTeam('builder-reviewer', false, 'First project');
      // The template's reviewer is Codex; a first project runs on Claude only.
      if (rec.draft.agents.some((a) => a.kind !== 'claude')) await api.saveTeam(rec.draft.id, rec.draft.revision, setKindAll(rec.draft, 'claude'));
      navigate(boardPath(rec.draft.id, 'design'));
      setup.startTour();
    } catch (e) { setError(failText(e)); setBusy(false); }
  };
  return (
    <Step title="Your first project">
      <p>Try a first team: a builder hands its work to a reviewer, both Claude agents. A short tour then points out the parts of the real screens. Nothing runs until you press Start; once started, the agents use your model sign-in and may cost money.</p>
      <p><button type="button" className="btn btn-primary" disabled={busy} onClick={() => void create()}>{busy ? 'Creating…' : 'Create the first project'}</button></p>
      {error && <p className="tone-red" role="alert">{error}</p>}
      <StepNav nav={nav} optional noContinue />
    </Step>
  );
}

function Finished({ setup }: { setup: Setup }) {
  const hosted = useHosted().kind === 'signed-in';
  return (
    <Step title="Setup is done">
      <p>{hosted ? 'You have a computer and a model connection.' : 'Agents can reach a model.'} Change these any time from Account.</p>
      {setup.requiredDone && <p><Link to="/" className="btn btn-primary">Go to Projects</Link></p>}
    </Step>
  );
}
