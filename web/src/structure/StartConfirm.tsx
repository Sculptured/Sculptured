import { effectiveFolder } from '../draft/folders';
import { api } from '../api';
import { useEffect, useRef, useState } from 'react';
import type { CompiledRig } from '../generated/CompiledRig';
import type { Runtime } from '../generated/Runtime';
import type { TeamDraft } from '../generated/TeamDraft';
import { nameForId, type NameMap } from '../vocab';
import { Btn, Icon } from '../shell/ui';
import { shortPath } from './FolderPicker';
import { plural } from '../format';
import { efficiencyNotes } from './startCost';

/** What starting a team with real agents does, said before it happens. */
export type StartConfirmation = {
  title: string; body: string; efficiency: string[]; team?: string;
  /** Said plainly when agents may work in the whole home folder. */
  broad?: string;
  /** Each agent's own limits, or that none are set. */
  limits?: string[];
};

/** Said in every Start confirmation: starting is never a one-way door. */
export const STOP_ANY_TIME = 'You can stop the team at any time: Stop team…, at the top of every project page, ends every agent. Their history is kept.';
/** What the person chose in the confirmation. */
export type StartChoice = { resume: boolean };

const list = (xs: string[]) => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} and ${xs.at(-1)}`);

type Starter = { id: string; name: string; kind: 'claude' | 'codex' | 'other'; folder: string | null };

function describe(agents: Starter[], handsTo: { from: string; to: string }[], home: string | null): StartConfirmation | null {
  const claude = agents.filter((a) => a.kind === 'claude');
  const codex = agents.filter((a) => a.kind === 'codex');
  if (claude.length === 0 && codex.length === 0) return null;
  const who = [claude.length ? plural(claude.length, 'Claude agent') : '', codex.length ? plural(codex.length, 'Codex agent') : ''].filter(Boolean);
  const accounts = [claude.length ? 'Claude' : '', codex.length ? 'Codex' : ''].filter(Boolean);
  const folders = [...new Set([...claude, ...codex].map((a) => (a.folder ? shortPath(a.folder, home) : '~ (home folder)')))];
  const where = folders.length === 1 ? folders[0] : `${folders.length} folders (${list(folders.slice(0, 3))}${folders.length > 3 ? ', …' : ''})`;
  // No folder, or the home folder itself: the agent can reach every file you own there.
  const atHome = [...claude, ...codex].filter((a) => !a.folder || a.folder === '~' || (home !== null && a.folder.replace(/\/+$/, '') === home.replace(/\/+$/, '')));
  const broad = atHome.length === 0 ? undefined
    : `${atHome.length === claude.length + codex.length ? (atHome.length === 1 ? 'This agent works' : 'These agents work') : `${list(atHome.map((a) => a.name))} ${atHome.length === 1 ? 'works' : 'work'}`} in your whole home folder (~), so ${atHome.length === 1 ? 'it' : 'they'} can read and change any file in it. To narrow this, choose a project folder on the Design board.`;
  // The first to start: agents no other agent of the team hands work to. They get their job at once.
  const ids = new Set(agents.map((a) => a.id));
  const heads = agents.filter((a) => !handsTo.some((c) => c.to === a.id && c.from !== a.id && ids.has(c.from)));
  return {
    title: `Start ${list(who)}?`,
    body: `They run in ${where} and use your ${list(accounts)} account${accounts.length > 1 ? 's' : ''}. First to start: ${heads.length ? list(heads.map((a) => a.name)) : 'unknown'}.`,
    efficiency: [],
    broad,
  };
}

const LIMIT_WORDS = (a: TeamDraft['agents'][number]) =>
  [a.advanced.token_limit ? 'tokens' : '', a.advanced.turn_limit ? 'turns' : '', a.advanced.minute_limit ? 'time' : ''].filter(Boolean);

/** Which agents have their own limits; when nothing limits the run at all, that is said too. */
export function limitNotes(d: TeamDraft): string[] {
  const limited = d.agents.filter((a) => a.kind !== 'practice' && LIMIT_WORDS(a).length > 0);
  if (limited.length > 0) return [`Agent limits: ${limited.map((a) => `${a.name.trim() || 'Unnamed agent'} (${LIMIT_WORDS(a).join(', ')})`).join('; ')}. At the soft value an agent is asked to wrap up; at the hard value it is stopped.`];
  const b = d.budget;
  const budget = !!b && ((b.plan?.length ?? 0) > 0 || !!b.tokens || !!b.minutes);
  return budget ? [] : ['No run budget or agent limits are set: agents work until they finish or you stop them.'];
}

/**
 * The confirmation a Start needs, or null when every agent is a Practice agent (free, no model
 * calls), which starts straight away. Real agents use the operator's own CLI logins.
 */
export function startConfirmation(d: TeamDraft | undefined, home: string | null): StartConfirmation | null {
  if (!d) return null;
  const handsTo = d.workflow ? d.agents.filter(a => a.id !== d.workflow!.dispatcher).map(a => ({ from: d.workflow!.dispatcher, to: a.id })) : d.connections.filter((c) => c.kind === 'hands-work-to');
  const c = describe(
    d.agents.map((a) => ({ id: a.id, name: a.name.trim() || 'Unnamed agent', kind: a.kind === 'practice' ? 'other' : a.kind, folder: effectiveFolder(d, a) })),
    handsTo,
    home,
  );
  return c && { ...c, efficiency: efficiencyNotes(d, handsTo), limits: limitNotes(d), team: d.id };
}

/** The same from a started team's compiled spec, for when its design has not loaded yet. */
export function specStartConfirmation(rig: CompiledRig, names: NameMap, home: string | null): StartConfirmation | null {
  const kind = (r: Runtime): Starter['kind'] => (r === 'claude-code' ? 'claude' : r === 'codex' ? 'codex' : 'other');
  const c = describe(
    rig.seats.map((s) => ({ id: s.id, name: nameForId(s.id, names), kind: kind(s.runtime), folder: s.cwd === '.' ? null : s.cwd })),
    rig.edges.filter((e) => e.kind === 'delegates_to'),
    home,
  );
  return c && { ...c, team: rig.name };
}

/** Agents with a last conversation to resume, once the dialog opens; empty when none or unknown. */
function useResumable(team: string | undefined) {
  const [agents, setAgents] = useState<string[]>([]);
  useEffect(() => {
    if (!team) return;
    let live = true;
    api.resumable(team).then((r) => live && setAgents(r.agents.map((a) => a.name)), () => {});
    return () => { live = false; };
  }, [team]);
  return agents;
}

/** Start fresh (default) or resume each agent's last conversation, offered when there is one to resume. */
function FreshOrResume({ agents, resume, setResume }: { agents: string[]; resume: boolean; setResume: (r: boolean) => void }) {
  if (agents.length === 0) return null;
  return (
    <fieldset className="pad start-mode">
      <legend className="fld-label">Conversations</legend>
      <label><input type="radio" name="start-mode" checked={!resume} onChange={() => setResume(false)} /> <b>Start fresh</b>: every agent begins a new conversation. Your files stay as they are.</label>
      <label><input type="radio" name="start-mode" checked={resume} onChange={() => setResume(true)} /> <b>Resume</b>: {plural(agents.length, 'agent')} ({list(agents.slice(0, 4))}{agents.length > 4 ? ', …' : ''}) continue their last conversation; the others start fresh.</label>
      {resume && <p className="tone-amber">A resumed agent keeps everything from its last run in its context and re-reads it on every step, so each step costs more.</p>}
    </fieldset>
  );
}

/** What a busy team may cost, under the confirmation. */
function CostNotes({ notes }: { notes: string[] }) {
  if (notes.length === 0) return null;
  return <ul className="pad start-cost tone-amber" aria-label="What this may cost">{notes.map((n) => <li key={n}>{n}</li>)}</ul>;
}

/** How far the agents reach, what limits them, and that a Start can always be undone by Stop. */
function ReachAndLimits({ c }: { c: StartConfirmation }) {
  return (
    <>
      {c.broad && <p className="pad tone-amber ux-broad-grant"><span aria-hidden="true">△ </span>{c.broad}</p>}
      {c.limits && c.limits.length > 0 && <ul className="pad ux-start-limits" aria-label="Limits">{c.limits.map((n) => <li key={n}>{n}</li>)}</ul>}
      <p className="pad muted ux-stop-anytime">{STOP_ANY_TIME}</p>
    </>
  );
}

/** Escape closes; every other key stays in the dialog, so board shortcuts don't fire behind it. */
function keys(onCancel: () => void) {
  return (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === 'Escape') {
      e.preventDefault();
      onCancel();
    }
  };
}

/** A modal asking before a costly Start. Cancel has focus, so Enter alone never starts. */
export function StartConfirm({ c, onConfirm, onCancel }: { c: StartConfirmation; onConfirm: (choice: StartChoice) => void; onCancel: () => void }) {
  const cancel = useRef<HTMLButtonElement>(null);
  const agents = useResumable(c.team);
  const [resume, setResume] = useState(false);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    cancel.current?.focus();
    return () => {
      if (prev && document.contains(prev)) prev.focus?.();
    };
  }, []);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="panel start-confirm" role="alertdialog" aria-modal="true" aria-labelledby="start-confirm-title" aria-describedby="start-confirm-body" onKeyDown={keys(onCancel)}>
        <div className="panel-bar">
          <span className="panel-title" id="start-confirm-title"><Icon name="play" size={15} /> {c.title}</span>
        </div>
        <p className="pad" id="start-confirm-body">{c.body}</p>
        <ReachAndLimits c={c} />
        <FreshOrResume agents={agents} resume={resume} setResume={setResume} />
        <CostNotes notes={c.efficiency} />
        <div className="start-confirm-actions">
          <Btn ref={cancel} onClick={onCancel}>Cancel</Btn>
          <Btn variant="primary" icon="play" onClick={() => onConfirm({ resume: resume && agents.length > 0 })}>Start team</Btn>
        </div>
      </div>
    </div>
  );
}

/** `ask(c, go)`: runs `go` at once when `c` is null, else after the operator confirms. */
export function useStartConfirm() {
  const [pending, setPending] = useState<{ c: StartConfirmation; go: (choice: StartChoice) => void } | null>(null);
  const ask = (c: StartConfirmation | null, go: (choice: StartChoice) => void) => (c ? setPending({ c, go }) : go({ resume: false }));
  const dialog = pending && (
    <StartConfirm
      c={pending.c}
      onCancel={() => setPending(null)}
      onConfirm={(choice) => {
        const go = pending.go;
        setPending(null);
        go(choice);
      }}
    />
  );
  return { ask, dialog };
}
