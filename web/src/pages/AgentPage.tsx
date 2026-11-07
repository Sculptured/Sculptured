import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { seatIdFor } from '../draft/ids';
import { effectiveModel } from '../draft/models';
import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EventRecord } from '../generated/EventRecord';
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
  if (draft && d) return `${KIND_LABEL[d.kind]} · ${modelLabel(d.kind, effectiveModel(draft, d).model)}`;
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

const tabOf = (tab: string | undefined): AgentTab | null => (TABS.some((t) => t.id === tab) ? (tab as AgentTab) : null);

/** The agent's address, following it when a move to another group changes it. */
function useSeatAddress(p: Project, key: string, tab: string | undefined) {
  const asked = seatFromKey(p.team, key);
  const d = agentOf(p.draft, asked);
  const seat = d && p.draft ? seatIdFor(p.draft, d.id)! : asked;
  const navigate = useNavigate();
  useEffect(() => {
    if (seat !== asked) navigate(agentPagePath(p.team, seat, tabOf(tab) ?? undefined), { replace: true });
  }, [seat, asked, p.team, tab, navigate]);
  return { seat, d };
}

/** What to show when no agent matches the address, or null when there is one. */
function missingState(p: Project, row: AgentRow | undefined, d: DraftAgent | undefined): { loading: boolean; unknown?: boolean } | null {
  if (!row && !d) return { loading: p.live ? false : p.started || (!p.draft && !p.t.loadError) };
  // In a started team's spec, but no status has arrived for it: say so, not "not started".
  if (row && !row.seat && !d && row.state.key !== 'planned') return { loading: false, unknown: true };
  return null;
}

/**
 * `/p/:team/agents/:agent[/:tab]`: one agent, one job per tab. Conversation is the default once the
 * team has started, Setup before.
 */
export function AgentPage() {
  const p = useProject();
  const shell = useShell();
  const { agent: key, tab } = useParams();
  const { seat, d } = useSeatAddress(p, key!, tab);
  const row = shell.byId[seat];
  const missing = missingState(p, row, d);
  if (missing) return <Missing p={p} seat={seat} loading={missing.loading} unknown={missing.unknown} />;
  const name = d?.name.trim() || row?.name || 'Unnamed agent';
  const current = tabOf(tab) ?? (row?.seat ? 'conversation' : 'setup');
  if (row?.seat) return <LiveAgent key={seat} p={p} seat={seat} row={row} d={d} name={name} tab={current} />;
  return <PlannedAgent p={p} seat={seat} name={name} row={row} d={d} tab={current} />;
}

/** An agent in the design that has not started: Setup and Questions work, the rest say "not started". */
function PlannedAgent({ p, seat, name, row, d, tab }: { p: Project; seat: string; name: string; row?: AgentRow; d?: DraftAgent; tab: AgentTab }) {
  return (
    <main className={`page agent-page ap-on-${tab}`}>
      <AgentHead p={p} seat={seat} name={name} row={row} d={d} tab={tab} />
      {tab === 'setup' ? <AgentSetup p={p} agentId={d?.id} /> : tab === 'questions' ? <AgentQuestions team={p.team} seat={seat} name={name} feed={[]} /> : <NotStarted p={p} seat={seat} tab={tab} />}
    </main>
  );
}

/** Hand over again lives on Activity (and in Ctrl K), unless it is the obvious next step: the CLI
 * was signed out, and after signing in, handing over again is how the agent tries again. */
function useHandOver(p: Project, seat: string, row: AgentRow, tab: AgentTab, feed: EventRecord[]) {
  const inline = signedOut(row, feed);
  const [askHandOver, setAskHandOver] = useState(0);
  // Asked from Ctrl K on another tab: Activity opens and asks there, the same question as its button.
  const [askOnOpen, setAskOnOpen] = useState(false);
  const navigate = useNavigate();
  const ask = () => {
    if (inline || tab === 'activity') setAskHandOver((n) => n + 1);
    else { setAskOnOpen(true); navigate(agentPagePath(p.team, seat, 'activity')); }
  };
  return { inline, askHandOver, askOnOpen, clearAsk: () => setAskOnOpen(false), ask };
}

/** A running (or once-run) agent: one set of controls for its Conversation and Activity. */
function LiveAgent({ p, seat, row, d, name, tab }: { p: Project; seat: string; row: AgentRow; d?: DraftAgent; name: string; tab: AgentTab }) {
  const shell = useShell();
  const heading = useRef<HTMLHeadingElement>(null);
  const c = useAgentControls(row.seat!, heading);
  const feed = p.live?.feed[seat] ?? [];
  const h = useHandOver(p, seat, row, tab, feed);
  shell.refs.controls.current = { ...c, handOver: (row.handsTo ?? []).length === 0 ? null : { reason: handOverReason(row), ask: h.ask } };
  useEffect(() => () => { shell.refs.controls.current = null; }, [shell.refs.controls]);
  const [search, setSearch] = useState<HTMLElement | null>(null);
  return (
    <main className={`page agent-page ap-on-${tab}`}>
      <AgentHead p={p} seat={seat} name={name} row={row} d={d} tab={tab} headingRef={heading} tools={tab === 'conversation' ? <span className="pane-search" ref={setSearch} /> : null} />
      <LiveTab p={p} seat={seat} row={row} d={d} name={name} tab={tab} c={c} feed={feed} h={h} search={search} />
    </main>
  );
}

type LiveTabProps = { p: Project; seat: string; row: AgentRow; d?: DraftAgent; name: string; tab: AgentTab; c: ReturnType<typeof useAgentControls>; feed: EventRecord[]; h: ReturnType<typeof useHandOver>; search: HTMLElement | null };

function LiveTab({ p, seat, row, d, name, tab, c, feed, h, search }: LiveTabProps) {
  const epoch = p.live?.epoch ?? 0;
  switch (tab) {
    case 'conversation': return <AgentConversation a={row} c={c} feed={feed} epoch={epoch} searchSlot={search} handOverInline={h.inline} askHandOver={h.inline ? h.askHandOver : 0} />;
    case 'setup': return <AgentSetup p={p} agentId={d?.id} />;
    case 'questions': return <AgentQuestions team={p.team} seat={seat} name={name} feed={feed} />;
    case 'activity': return <AgentActivity a={row} c={c} feed={feed} epoch={epoch} handOverInline={h.inline} askHandOver={h.askHandOver} askOnOpen={h.askOnOpen} onAsked={h.clearAsk} />;
    default: return null;
  }
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
