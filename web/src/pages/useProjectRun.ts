import { useState } from 'react';
import { api, ApiError } from '../api';
import { errorsOf, startReason } from '../draft/issues';
import { errorText, issuesOf, STARTING_REASON } from '../errors';
import type { Refusal } from '../shell/StartRefused';
import type { StopFailure } from '../generated/StopFailure';
import { useShell } from '../shell/ShellContext';
import { startConfirmation, useStartConfirm } from '../structure/StartConfirm';
import { useHome } from '../structure/FolderPicker';
import { hasProcess } from '../status';
import { useRequestOnce } from '../useRequestOnce';
import type { Project } from './ProjectContext';

/** What the live stream says about running: a process, launching, and anything at all live. */
export function liveFacts(p: Project) {
  const views = p.live?.snapshot ? p.live.snapshot.rig.seats.map((s) => p.live!.seats[s.id]).filter(Boolean) : [];
  const anyLive = views.some((s) => hasProcess(s.lifecycle) || s.lifecycle === 'attention');
  const processes = views.some((s) => hasProcess(s.lifecycle));
  const starting = p.live?.snapshot?.desired === 'running' && views.some((s) => s.lifecycle === 'launching' || s.lifecycle === 'planned');
  return { views, anyLive, processes, starting: !!starting, live: anyLive || !!starting };
}

/**
 * Start team: saves the design first, asks before a costly start (the same confirmation as
 * before), and starts the latest saved revision. A team started from a file cannot start here.
 */
export function useStart(p: Project, onStarted: () => void) {
  const shell = useShell();
  const home = useHome();
  const confirm = useStartConfirm();
  const once = useRequestOnce();
  const [starting, setStarting] = useState(false);
  // Why the last start was refused (a dialog says it on every page); `resume` is what Start anyway repeats.
  const [refusal, setRefusal] = useState<(Refusal & { resume: boolean }) | null>(null);
  const f = liveFacts(p);
  // Nothing has a process (stopped, or lost in a restart): it can be started again.
  const live = f.processes || f.starting;
  const reason = startBlock(p, live, starting);
  const go = async (overPlanLimit: boolean, resume: boolean) => {
    setStarting(true); setRefusal(null);
    try {
      const saved = await p.t.flush();
      if (!saved.ok) return;
      const r = overPlanLimit ? await api.startTeam(p.team, saved.revision, undefined, true, resume) : await once((id) => api.startTeam(p.team, saved.revision, id, false, resume));
      shell.setStartWarnings(p.team, r.warnings ?? []);
      shell.reloadTeams();
      void p.t.refresh();
      onStarted();
    } catch (e) {
      startFailed(e, p);
      setRefusal({ text: errorText(e), issues: issuesOf(e), code: e instanceof ApiError ? e.code : undefined, resume });
    } finally {
      setStarting(false);
    }
  };
  const ask = () => { if (!reason && p.draft) confirm.ask(startConfirmation(p.draft, home), (c) => void go(false, c.resume)); };
  return { reason, ask, starting, refusal, close: () => setRefusal(null), anyway: () => void go(true, refusal?.resume ?? false), dialog: confirm.dialog, live };
}

function startFailed(e: unknown, p: Project) {
  if (!(e instanceof ApiError)) return;
  if (e.issues) p.t.setIssues(e.issues);
  if (e.code === 'team_running') void p.t.refresh();
}

/** Why Start cannot run now, in words; null when it can. */
export function startBlock(p: Project, live: boolean, starting: boolean): string | null {
  if (!p.builtHere) return 'This team was started from a file; start it again from its file.';
  if (live) return 'The team is already running.';
  if (p.startBlock) return p.startBlock;
  if (!p.draft) return 'Loading…';
  return startReason({ issues: p.issues, saveState: p.t.saveState, running: false, starting, empty: p.draft.agents.length === 0 });
}

/** Stop team…: asks first (Stop team, or Stop everything now); agents that could not be stopped are listed. */
export function useStop(p: Project) {
  const shell = useShell();
  const once = useRequestOnce();
  const [asking, setAsking] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [failures, setFailures] = useState<StopFailure[]>([]);
  const { live, starting, anyLive } = liveFacts(p);
  const reason = starting ? STARTING_REASON : !live ? 'The team is not running.' : null;
  const run = async (fn: () => Promise<void>) => {
    setError(null); setFailures([]); setStopping(true);
    try { await fn(); setAsking(false); } catch (e) { setError(errorText(e)); } finally { setStopping(false); }
  };
  const stop = () => run(async () => {
    const out = await once((id) => (shell.onStopTeam ? shell.onStopTeam(p.team, id) : p.builtHere ? api.stopTeam(p.team, id) : api.stopRig(p.team, id)));
    setFailures(out?.failed ?? []);
  });
  const now = () => run(async () => { await api.stopTeamNow(p.team); });
  return { live, anyLive, reason, asking, ask: () => { if (live && !reason) setAsking(true); }, cancel: () => setAsking(false), stopping, stop, now, error, failures, clear: () => { setError(null); setFailures([]); } };
}

/** Problems that block Start, for the status words. */
export const blockingCount = (p: Project) => errorsOf(p.issues).length;

/** Why Start cannot run right now, as the project bar says it. */
export const startReasonOf = (p: Project) => { const f = liveFacts(p); return startBlock(p, f.processes || f.starting, false); };
