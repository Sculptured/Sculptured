import { useEffect, useId, useRef } from 'react';
import { Link, NavLink, useLocation, useNavigate } from 'react-router-dom';
import type { DraftIssue } from '../generated/DraftIssue';
import { agentsPath, boardPath, questionsPath, resultsPath, settingsPath } from '../paths';
import { DismissibleError } from '../shell/DismissibleError';
import { CliReadiness, cliNeeds } from '../shell/CliReadiness';
import { StartRefused } from '../shell/StartRefused';
import { useShell } from '../shell/ShellContext';
import { Btn } from '../shell/ui';
import { RenameProject } from '../structure/RenameProject';
import { nameForId, WORDS } from '../vocab';
import { useProject, type Project } from './ProjectContext';
import { draftStatus, teamStatus } from './status';
import { StatusBadge } from './StatusBadge';
import { blockingCount, useStart, useStop } from './useProjectRun';

/** The project's pages, as tabs. */
function Tabs({ team }: { team: string }) {
  // An agent's own page belongs to Agents; a workflow page belongs to the Board.
  const { pathname } = useLocation();
  const onWorkflow = pathname.includes('/workflow/');
  const tab = (to: string, label: string, also = false, tour?: string) => (
    <NavLink to={to} className={({ isActive }) => `pb-tab${isActive || also ? ' on' : ''}`} aria-current={also ? 'page' : undefined} data-tour={tour}>{label}</NavLink>
  );
  return (
    <nav className="pb-tabs" aria-label="Project">
      {tab(boardPath(team), 'Board', onWorkflow)}
      {tab(agentsPath(team), 'Agents')}
      {tab(questionsPath(team), 'Questions', false, 'questions')}
      {tab(resultsPath(team), 'Results')}
      {tab(settingsPath(team), 'Settings')}
    </nav>
  );
}

/**
 * Under the top bar on every project page: the project's name, its pages as tabs, its status
 * (shape plus words) and the one primary action, Start team or Stop team…
 */
export function ProjectBar() {
  const p = useProject();
  const shell = useShell();
  const navigate = useNavigate();
  const ids = useId();
  const start = useStart(p, () => navigate(boardPath(p.team, 'run')));
  const stop = useStop(p);
  usePendingAction(p, start.ask, stop.ask, start.reason, stop.live);
  // The palette's handle on Start and Stop, current at every render.
  shell.refs.run.current = { live: stop.live, startReason: start.reason, stopReason: stop.reason, askStart: start.ask, askStop: stop.ask };
  useEffect(() => () => { shell.refs.run.current = null; }, [shell.refs.run]);
  const status = !p.started && p.builtHere ? draftStatus(p.draft ? blockingCount(p) : null) : teamStatus(p.row);
  const fix = !p.started && blockingCount(p) > 0;
  return (
    <div className="project-head">
      <div className="project-bar">
        <h1 className="pb-name" title={p.title}>{p.draft ? <RenameProject draft={p.draft} edit={p.t.edit} /> : p.title}</h1>
        <Tabs team={p.team} />
        <div className="pb-right">
          {fix ? (
            <Link to={boardPath(p.team, 'design')} id={`${ids}-why`} className="pb-status-link" data-tip={start.reason ?? undefined}><StatusBadge s={status} /></Link>
          ) : (
            <span id={`${ids}-why`}><StatusBadge s={status} /></span>
          )}
          {start.reason && <span id={`${ids}-start`} className="sr-only">Start team: {start.reason}</span>}
          <Primary p={p} start={start} stop={stop} why={`${ids}-start`} />
        </div>
      </div>
      <Notices p={p} start={start} stop={stop} />
      {start.dialog}
    </div>
  );
}

type StartApi = ReturnType<typeof useStart>;
type StopApi = ReturnType<typeof useStop>;

/** At most one primary action: Stop team… while anything runs, else Start team (disabled with its reason). */
function Primary({ p, start, stop, why }: { p: Project; start: StartApi; stop: StopApi; why: string }) {
  const ids = useId();
  const stopBtn = useRef<HTMLButtonElement>(null);
  // Closing the confirmation gives focus back to Stop team…
  const wasAsking = useRef(false);
  useEffect(() => {
    if (wasAsking.current && !stop.asking) stopBtn.current?.focus();
    wasAsking.current = stop.asking;
  }, [stop.asking]);
  // Stop while anything runs; Start when nothing has a process (a team lost in a restart offers Start).
  if (start.live || (stop.live && !p.builtHere))
    return (
      <span className="pb-action">
        <Btn ref={stopBtn} danger icon="stop" onClick={stop.ask} aria-expanded={stop.asking} disabled={!!stop.reason} aria-describedby={stop.reason ? `${ids}-stop` : undefined} aria-label={WORDS.stopTeam}>
          <span>Stop<span className="wide-only"> team</span>…</span>
        </Btn>
        {stop.reason && <span id={`${ids}-stop`} className="reason">{stop.reason}</span>}
      </span>
    );
  if (!p.builtHere) return null;
  return (
    <Btn variant="primary" icon="play" className="start-btn" data-tour="start" onClick={start.ask} disabled={!!start.reason} aria-describedby={start.reason ? why : undefined} aria-label="Start team" tip={start.reason ?? undefined}>
      <span>Start<span className="wide-only"> team</span></span>
    </Btn>
  );
}

/** Confirmations and outcomes of Start and Stop, in one quiet strip under the bar. */
function Notices({ p, start, stop }: { p: Project; start: StartApi; stop: StopApi }) {
  const shell = useShell();
  const warnings = shell.startWarnings[p.team] ?? [];
  return (
    <>
      {stop.asking && <StopConfirm p={p} stop={stop} />}
      <StartRefused refusal={start.refusal} onClose={start.close} onAnyway={start.anyway}>
        {start.refusal?.code === 'cli_not_ready' && p.draft && <CliReadiness need={cliNeeds(p.draft)} on={{ team: p.team }} onReady={() => { start.close(); start.ask(); }} />}
        {start.refusal && start.refusal.code !== 'plan_limit' && start.refusal.code !== 'cli_not_ready' && p.builtHere && <p>Fix it on the <Link to={boardPath(p.team, 'design')} onClick={start.close}>Design board</Link>.</p>}
      </StartRefused>
      {stop.error && <DismissibleError className="pb-note tone-red">{stop.error}</DismissibleError>}
      {stop.failures.length > 0 && (
        <DismissibleError className="pb-note stop-failures">
          <p>{WORDS.agentsNotStopped(stop.failures.length)}</p>
          <ul aria-label={WORDS.agentsNotStoppedLabel}>
            {stop.failures.map((f) => <li key={f.seat}>{nameForId(f.seat, p.row?.names ?? {})} — <span data-raw-evidence>{f.reason}</span></li>)}
          </ul>
        </DismissibleError>
      )}
      {warnings.length > 0 && <StartWarnings warnings={warnings} onDismiss={() => shell.setStartWarnings(p.team, [])} />}
    </>
  );
}

function StartWarnings({ warnings, onDismiss }: { warnings: DraftIssue[]; onDismiss: () => void }) {
  return (
    <div className="pb-note start-warnings tone-amber" role="status" aria-label="Start warnings">
      <span>Started with {warnings.length === 1 ? 'a warning' : `${warnings.length} warnings`}: {warnings.map((w) => w.message).join(' · ')}</span>
      <Btn size="sm" variant="ghost" onClick={onDismiss}>Dismiss</Btn>
    </div>
  );
}

/** Stop team… asks first. Esc or Cancel keeps the team running. */
function StopConfirm({ p, stop }: { p: Project; stop: StopApi }) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => cancel.current?.focus(), []);
  return (
    <div role="group" aria-label={WORDS.confirmStopTeam} className="pb-note pb-confirm confirm"
      onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); stop.cancel(); } }}>
      <p>{WORDS.stopTeamConsequence(p.title)}</p>
      <Btn danger onClick={() => void stop.stop()} disabled={stop.stopping}>{WORDS.confirmStopTeam}</Btn>
      <Btn danger onClick={() => void stop.now()} disabled={stop.stopping} tip="Interrupts every agent and the commands it is running at once, like Ctrl+C in a terminal; anything still running after 2 seconds is ended.">Stop everything now</Btn>
      {stop.stopping && <span className="reason">{WORDS.stoppingTeam(p.title)}</span>}
      <Btn ref={cancel} onClick={stop.cancel}>Cancel</Btn>
    </div>
  );
}

/** Start or Stop asked for from elsewhere (the Projects list): run it here once the project has loaded. */
function usePendingAction(p: Project, askStart: () => void, askStop: () => void, startReason: string | null, live: boolean) {
  const { pending } = useShell();
  useEffect(() => {
    const want = pending.current;
    if (!want || want.team !== p.team) return;
    if (Date.now() > want.until) { pending.current = null; return; }
    if (want.action === 'start' && p.draft && startReason !== 'Loading…') { pending.current = null; askStart(); }
    if (want.action === 'stop' && live) { pending.current = null; askStop(); }
  });
}
