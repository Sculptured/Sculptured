import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { seatIdFor } from '../draft/ids';
import { effectiveModel } from '../draft/models';
import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';
import { agentPagePath, boardPath, seatFromKey, type AgentTab } from '../paths';
import { useAgentControls } from '../shell/AgentControls';
import { handOverReason } from '../shell/HandoffActions';
import { NOT_STARTED, type AgentRow } from '../shell/model';
import { useShell } from '../shell/ShellContext';
import { signedOut } from '../shell/SignInNeeded';
import { Btn, Icon, StateMark, VendorMark } from '../shell/ui';
import { kindVendor } from '../structure/Panels';
import { modelLabel } from '../structure/ModelControls';
import { KIND_LABEL, nameForId, WORDS } from '../vocab';
import { DismissibleError } from '../shell/DismissibleError';
import { AgentActivity } from './AgentActivity';
import { AgentConversation } from './AgentConversation';
import { AgentSetup } from './AgentSetup';
import { AgentQuestions } from '../questions/QuestionsPage';
import { useProject, type Project } from './ProjectContext';

const TABS: { id: AgentTab; label: string }[] = [
  { id: 'conversation', label: 'Conversation' },
  { id: 'setup', label: 'Setup' },
  { id: 'activity', label: 'Activity' },
  { id: 'questions', label: 'Questions' },
];

/** "Claude · Opus 4.1": what the agent runs on, from its design when there is one. */
function runsOn(draft: TeamDraft | null, d: DraftAgent | undefined, row: AgentRow | undefined): string {
  if (draft && d) return d.kind === 'practice' ? 'Practice · free' : `${KIND_LABEL[d.kind]} · ${modelLabel(d.kind, effectiveModel(draft, d).model)}`;
  const model = row?.seat?.model_observed ?? row?.seat?.model_requested;
  return row ? `${row.vendor.name}${model ? ` · ${model}` : ''}` : 'unknown';
}

type HeadProps = { p: Project; seat: string; name: string; row?: AgentRow; d?: DraftAgent; tab: AgentTab; headingRef?: RefObject<HTMLHeadingElement | null>; tools?: ReactNode };

/** Back to the board, who the agent is, its status and what it runs on; then its tabs. */
function AgentHead({ p, seat, name, row, d, tab, headingRef, tools }: HeadProps) {
  const shell = useShell();
  const v = d ? kindVendor(d.kind) : row!.vendor;
  return (
    <header className="ap-head">
      <div className="ap-title-row">
        <Btn variant="ghost" size="sm" icon="arrow-left" onClick={shell.back} kbd="esc" tip="Back to the board, as you left it">Board</Btn>
        <VendorMark v={v} size="md" />
        <h1 ref={headingRef} tabIndex={-1} className="ap-name">{name}</h1>
        <StateMark state={row?.state ?? NOT_STARTED} pill />
        <span className="ap-runs-on muted"><span className="sr-only">Runs on </span>{runsOn(p.draft, d, row)}</span>
      </div>
      <div className="ap-tab-row">
        <nav className="ap-tabs" aria-label="Agent pages">
          {TABS.map((t) => (
            <Link key={t.id} to={agentPagePath(p.team, seat, t.id)} className={`ap-tab${tab === t.id ? ' on' : ''}`} aria-current={tab === t.id ? 'page' : undefined}>{t.label}</Link>
          ))}
        </nav>
        <span className="ap-tools">{tools}</span>
      </div>
    </header>
  );
}

/** The design's agent at this address: by its full address, else by its own id (it may have changed group). */
function agentOf(draft: TeamDraft | null, seat: string): DraftAgent | undefined {
  if (!draft) return undefined;
  const member = seat.split('@')[0].split('.').at(-1);
  return draft.agents.find((a) => seatIdFor(draft, a.id) === seat) ?? draft.agents.find((a) => a.id === member);
}

/**
 * `/p/:team/agents/:agent[/:tab]`: one agent, one job per tab. Conversation is the default once the
 * team has started, Setup before.
 */
export function AgentPage() {
  const p = useProject();
  const shell = useShell();
  const { agent: key, tab } = useParams();
  const asked = seatFromKey(p.team, key!);
  const d = agentOf(p.draft, asked);
  // Moving an agent to another group changes its address: follow it to the new one.
  const seat = d && p.draft ? seatIdFor(p.draft, d.id)! : asked;
  const navigate = useNavigate();
  useEffect(() => {
    if (seat !== asked) navigate(agentPagePath(p.team, seat, TABS.some((t) => t.id === tab) ? (tab as AgentTab) : undefined), { replace: true });
  }, [seat, asked, p.team, tab, navigate]);
  const row = shell.byId[seat];
  if (!row && !d) return <Missing p={p} seat={seat} loading={p.live ? false : p.started || (!p.draft && !p.t.loadError)} />;
  // In a started team's spec, but no status has arrived for it: say so, not "not started".
  if (row && !row.seat && !d && row.state.key !== 'planned') return <Missing p={p} seat={seat} loading={false} unknown />;
  const name = d?.name.trim() || row?.name || 'Unnamed agent';
  const chosen = TABS.some((t) => t.id === tab) ? (tab as AgentTab) : null;
  const current = chosen ?? (row?.seat ? 'conversation' : 'setup');
  if (row?.seat) return <LiveAgent key={seat} p={p} seat={seat} row={row} d={d} name={name} tab={current} />;
  return (
    <main className={`page agent-page ap-on-${current}`}>
      <AgentHead p={p} seat={seat} name={name} row={row} d={d} tab={current} />
      {current === 'setup' ? <AgentSetup p={p} agentId={d?.id} /> : current === 'questions' ? <AgentQuestions team={p.team} seat={seat} name={name} feed={[]} /> : <NotStarted p={p} seat={seat} tab={current} />}
    </main>
  );
}

/** A running (or once-run) agent: one set of controls for its Conversation and Activity. */
function LiveAgent({ p, seat, row, d, name, tab }: { p: Project; seat: string; row: AgentRow; d?: DraftAgent; name: string; tab: AgentTab }) {
  const shell = useShell();
  const heading = useRef<HTMLHeadingElement>(null);
  const c = useAgentControls(row.seat!, heading);
  const feed = p.live?.feed[seat] ?? [];
  // Hand over again lives on Activity (and in Ctrl K), unless it is the obvious next step: the CLI
  // was signed out, and after signing in, handing over again is how the agent tries again.
  const inline = signedOut(row, feed);
  const [askHandOver, setAskHandOver] = useState(0);
  // Asked from Ctrl K on another tab: Activity opens and asks there, the same question as its button.
  const [askOnOpen, setAskOnOpen] = useState(false);
  const navigate = useNavigate();
  const ask = () => {
    if (inline || tab === 'activity') setAskHandOver((n) => n + 1);
    else { setAskOnOpen(true); navigate(agentPagePath(p.team, seat, 'activity')); }
  };
  shell.refs.controls.current = { ...c, handOver: (row.handsTo ?? []).length === 0 ? null : { reason: handOverReason(row), ask } };
  useEffect(() => () => { shell.refs.controls.current = null; }, [shell.refs.controls]);
  const [search, setSearch] = useState<HTMLElement | null>(null);
  return (
    <main className={`page agent-page ap-on-${tab}`}>
      <AgentHead p={p} seat={seat} name={name} row={row} d={d} tab={tab} headingRef={heading} tools={tab === 'conversation' ? <span className="pane-search" ref={setSearch} /> : null} />
      {tab === 'conversation' && <AgentConversation a={row} c={c} feed={feed} epoch={p.live?.epoch ?? 0} searchSlot={search} handOverInline={inline} askHandOver={inline ? askHandOver : 0} />}
      {tab === 'setup' && <AgentSetup p={p} agentId={d?.id} />}
      {tab === 'questions' && <AgentQuestions team={p.team} seat={seat} name={name} feed={feed} />}
      {tab === 'activity' && <AgentActivity a={row} c={c} feed={feed} epoch={p.live?.epoch ?? 0} handOverInline={inline} askHandOver={askHandOver} askOnOpen={askOnOpen} onAsked={() => setAskOnOpen(false)} />}
    </main>
  );
}

/** Conversation and Activity before the team has started: nothing has happened yet. */
function NotStarted({ p, seat, tab }: { p: Project; seat: string; tab: AgentTab }) {
  return (
    <section className="ap-body ap-empty" aria-label={tab === 'conversation' ? 'Conversation' : 'Activity'}>
      <Icon name={tab === 'conversation' ? 'message' : 'activity'} size={22} className="muted" />
      <h2>Not started yet</h2>
      <p className="muted">{tab === 'conversation' ? 'Start the team to talk to this agent.' : 'Its progress, files and raw events show here once the team has started.'}</p>
      <Link className="btn" to={agentPagePath(p.team, seat, 'setup')}>Open Setup</Link>
    </section>
  );
}

function Missing({ p, seat, loading, unknown = false }: { p: Project; seat: string; loading: boolean; unknown?: boolean }) {
  const name = nameForId(seat, p.row?.names ?? {});
  return (
    <main className="page agent-page">
      <section className="ap-body ap-empty" aria-label="Agent">
        {loading ? <p role="status" className="muted">Loading agent…</p> : (
          <>
            <DismissibleError className="tone-amber">{unknown ? WORDS.statusUnknown(name) : WORDS.notInTeam(name, p.title)}</DismissibleError>
            {!unknown && <p className="muted">It may have been deleted or renamed.</p>}
            <Link className="btn" to={boardPath(p.team)}>Back to the board</Link>
          </>
        )}
      </section>
    </main>
  );
}
