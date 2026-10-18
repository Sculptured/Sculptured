import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { api } from '../api';
import type { GitSnapshot } from '../generated/GitSnapshot';
import { plural } from '../format';
import type { AgentRow } from './model';
import { allAgentsFinished, checkLine, finishedKey, finishedLine, lastReply } from './runFinished';
import { Icon, StateMark } from './ui';

const STILL_RUNNING = 'Every agent finished its turn; the team is still running.';

/** How long every agent must stay finished before the run counts as finished: a handoff can be in flight. */
export const SETTLE_MS = 1500;

type Files = { state: 'loading' } | { state: 'known'; git: GitSnapshot } | { state: 'unknown' } | { state: 'no-folder' };

/** True once `on` has held for `ms`; false again the moment it stops holding. */
export function useSettled(on: boolean, ms: number): boolean {
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (!on) { setSettled(false); return; }
    const t = setTimeout(() => setSettled(true), ms);
    return () => clearTimeout(t);
  }, [on, ms]);
  return on && settled;
}

/** What changed in the project folder, as git sees it now (filesystem-observed). Read again when `key` changes. */
function useFiles(teamId: string | undefined, key: string, folder: string | null | undefined): Files {
  const [files, setFiles] = useState<Files>({ state: 'unknown' });
  useEffect(() => {
    if (folder === null) { setFiles({ state: 'no-folder' }); return; }
    if (!teamId) return;
    let live = true;
    setFiles({ state: 'loading' });
    api.gitStatus(teamId).then((git) => live && setFiles({ state: 'known', git }), () => live && setFiles({ state: 'unknown' }));
    return () => { live = false; };
  }, [teamId, key, folder]);
  return files;
}

function FilesLine({ files, teamId }: { files: Files; teamId?: string }) {
  if (files.state === 'loading') return <p className="muted">Checking the project folder…</p>;
  if (files.state !== 'known' || !files.git.repository)
    return (
      <p>
        <span className="ux-unknown">unknown</span>{' '}
        <span className="muted">
          {files.state === 'no-folder' ? '(this team has no project folder to look in)' : files.state === 'known' ? '(the project folder is not a git repository)' : '(Sculptured could not read the project folder)'}
        </span>
      </p>
    );
  const n = files.git.files.length;
  return (
    <p>
      {n === 0 ? 'None' : plural(n, 'file')} in the project folder{' '}
      <span className="ux-prov" title="Read from git in the project folder; it can include changes made before this run.">filesystem-observed</span>
      {teamId && n > 0 && <> · <Link to={`/p/${encodeURIComponent(teamId)}/git`}>Open Git changes</Link></>}
    </p>
  );
}

/** What is verified, in one sentence; never more than the checks show. `anyCheck`: some agent has a check set. */
export function verdict(verified: number, total: number, anyCheck = true): string {
  if (verified > 0) return `Verified: ${verified} of ${plural(total, 'agent')} passed the check Sculptured ran. Everything else above is what the agents reported.`;
  return anyCheck
    ? 'Verified: nothing. No agent has a passing check, so whether the work is right is unknown until you review it.'
    : 'Verified: nothing. No agent has a check set, so whether the work is right is unknown until you review it.';
}

const REPORTED_TIP = "agent-reported: the agent's own words. Saying it is done is not proof the work is right.";

/** One agent: its name (opens it), its state, and what it said (its words, agent-reported, marked by a small quote icon). */
function AgentLine({ a, onOpen }: { a: AgentRow; onOpen: (id: string) => void }) {
  const said = lastReply(a);
  return (
    <li>
      <div className="ux-rf-head">
        <button type="button" className="link ux-rf-name" onClick={() => onOpen(a.id)} aria-label={`Open ${a.name}`} data-tip="Open the agent: its full reply, and a message box">{a.name}</button>
        <StateMark state={a.state} pill />
      </div>
      <p className="ux-rf-said">
        <span className="ux-prov-mark" data-tip={REPORTED_TIP}><Icon name="quote" size={12} /><span className="sr-only">agent-reported: </span></span>
        {said ? <q data-raw-evidence>{said}</q> : <span className="ux-unknown">no reply captured</span>}
      </p>
    </li>
  );
}

/** Each agent's check: only one Sculptured ran and that passed is verified. Then the verdict, once. */
function Checks({ agents }: { agents: AgentRow[] }) {
  const verified = agents.filter((a) => checkLine(a).verified).length;
  const any = agents.some((a) => !!a.seat?.check);
  return (
    <section className="rs-sec" aria-label="Checks">
      <h3>Checks</h3>
      {any && (
        <ul className="rs-checks">
          {agents.map((a) => {
            const c = checkLine(a);
            return (
              <li key={a.id} className={`ux-rf-check ${c.tone}`}>
                <Icon name={c.icon} size={12} className="ux-rf-check-icon" /><strong>{a.name}</strong> {c.word}
                {c.verified && <span className="ux-prov" title="Sculptured ran this command after the agent's turn; exit 0."> verified</span>}
              </li>
            );
          })}
        </ul>
      )}
      <p className="ux-rf-verdict">{verdict(verified, agents.length, any)}</p>
    </section>
  );
}

type Props = {
  rig: string;
  agents: AgentRow[];
  teamId?: string;
  /** The team's project folder: null when none is chosen, undefined when the design is not known. */
  folder?: string | null;
  onOpen: (id: string) => void;
};

/** The heading and one honest sentence: finished (turns ended, nothing waits), still working, or stopped. */
function heading(agents: AgentRow[], finished: boolean): { title: string; line: string } {
  if (finished) return { title: 'Run finished', line: STILL_RUNNING };
  const working = agents.filter((a) => a.state.key === 'working').length;
  if (agents.some((a) => a.seat?.lifecycle === 'running'))
    return { title: 'Run in progress', line: `${plural(working, 'agent')} working now. What each agent reported so far:` };
  return { title: 'Last run', line: 'The team is not running. What each agent reported last:' };
}

/**
 * The run summary: what each agent reported (its words, not verified), the files git sees changed,
 * and the check Sculptured ran for each (verified only when it passed); anything not known says unknown.
 */
export function RunSummary({ rig, agents, teamId, folder, onOpen }: Props) {
  const finished = useSettled(allAgentsFinished(agents), SETTLE_MS);
  const files = useFiles(teamId, finished ? finishedKey(agents) : 'now', folder);
  const h = heading(agents, finished);
  return (
    <div id={`run-summary-${rig}`} className="ux-run-finished run-summary">
      <section className="rs-sec" aria-label={h.title}>
        <header><h2><Icon name={finished ? 'check-circle' : 'activity'} size={17} className="ux-rf-title-icon" />{h.title}</h2><p className="muted">{h.line}</p></header>
        <ul className="ux-rf-agents" aria-label="What each agent did">
          {agents.map((a) => <AgentLine key={a.id} a={a} onOpen={onOpen} />)}
        </ul>
      </section>
      <section className="rs-sec" aria-label="Files changed">
        <h3>Files changed</h3>
        <FilesLine files={files} teamId={teamId} />
      </section>
      <Checks agents={agents} />
    </div>
  );
}

/** On the Run board, once every agent has stayed finished: one slim line that leads to Results. */
export function RunFinishedBar({ agents, href }: { agents: AgentRow[]; href: string }) {
  const finished = useSettled(allAgentsFinished(agents), SETTLE_MS);
  if (!finished) return null;
  return (
    <Link to={href} className="ux-rf-bar run-finished-bar" aria-label={`${finishedLine(agents)}. View results`}>
      <Icon name="check-circle" size={15} className="ux-rf-bar-icon" />
      <span className="ux-rf-bar-text">{finishedLine(agents)}</span>
      <span className="rf-view">View results</span>
      <Icon name="arrow-right" size={15} />
    </Link>
  );
}
