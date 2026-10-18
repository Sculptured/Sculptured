import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Outlet, useLocation, useMatch, useNavigate } from 'react-router-dom';
import { isTextField } from '../dom';
import type { DraftIssue } from '../generated/DraftIssue';
import type { StopRigOutcome } from '../generated/StopRigOutcome';
import type { View } from '../keys';
import type { Layout } from '../layout';
import { agentsPath, boardPath, seatFromKey, type AgentTab } from '../paths';
import { markSeen } from '../seen';
import type { BoardHandle } from '../structure/StructureBoard';
import type { OpenStream } from '../useRigStream';
import type { NameMap } from '../vocab';
import type { AgentControls } from './AgentControls';
import { AppearanceControl } from './AppearanceControl';
import { BroadcastDialog } from './Broadcast';
import { CommandPalette, type Scope } from './CommandPalette';
import { cycleAgent, useShellCommands } from './shellCommands';
import { HelpOverlay } from './HelpOverlay';
import type { GraphHandle } from './LiveGraph';
import { useDetailsMenus } from './menus';
import { agentPath, type AgentRow } from './model';
import { ShellContext, type Pending, type RunHandle, type Shell as ShellValue } from './ShellContext';
import { Toasts } from './Toasts';
import { TopBar } from './TopBar';
import { useShellData } from './useShellData';
import { useToasts } from './useToasts';

type Props = {
  openStream?: OpenStream;
  /** Persisted positions per team. */
  layouts?: Record<string, Layout>;
  /** Names from team drafts; they win over spec labels. */
  names?: Record<string, NameMap>;
  onStopTeam?: (rig: string, requestId: string) => Promise<StopRigOutcome>;
};

const NO_LAYOUTS: Record<string, Layout> = {};
const NO_NAMES: Record<string, NameMap> = {};

/** Where the app is: the open project, agent and page. */
function useWhere() {
  const project = useMatch('/p/:team/*');
  const agent = useMatch('/p/:team/agents/:agent/*');
  const team = project?.params.team;
  const openAgent = team && agent?.params.agent ? seatFromKey(team, agent.params.agent) : undefined;
  const page = project?.params['*']?.split('/')[0] ?? '';
  const tab = agent?.params['*']?.split('/')[0];
  const agentTab: AgentTab | undefined = tab === 'conversation' || tab === 'setup' || tab === 'activity' ? tab : undefined;
  return { team, openAgent, page, agentTab };
}

function viewOf(pathname: string, page: string, openAgent: string | undefined, boardView: View | null): View {
  if (pathname === '/') return 'home';
  if (openAgent) return 'agent';
  if (page === 'board') return boardView ?? 'structure';
  if (page === 'agents' || page === 'results' || page === 'settings' || page === 'workflow') return page;
  return 'other';
}

/** The refs pages register so the palette and keys can reach them. */
function useShellRefs() {
  return {
    board: useRef<BoardHandle | null>(null),
    run: useRef<RunHandle | null>(null),
    graph: useRef<GraphHandle | null>(null),
    controls: useRef<AgentControls | null>(null),
    composer: useRef<HTMLTextAreaElement | null>(null),
  };
}

/** Agents picked with Shift+click, for the open team only. */
function usePicks(team: string | undefined, byId: Record<string, AgentRow>) {
  const [picks, setPicks] = useState<{ rig: string; ids: string[] }>({ rig: '', ids: [] });
  const picked = useMemo(() => (picks.rig === team ? picks.ids.filter((id) => byId[id]?.rig === team) : []), [picks, team, byId]);
  const pick = useCallback(
    (id: string) =>
      setPicks((cur) => {
        const ids = cur.rig === team ? cur.ids : [];
        return { rig: team ?? '', ids: ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id] };
      }),
    [team],
  );
  const setPicked = useCallback((ids: string[]) => setPicks({ rig: team ?? '', ids }), [team]);
  return { picked, pick, setPicked };
}

/**
 * The app frame: one top bar, then the page. It holds what every page shares (the teams, the
 * open agent, the palette, messages to several agents, toasts and the global keys).
 */
export function Shell({ openStream, layouts: givenLayouts = NO_LAYOUTS, names: givenNames = NO_NAMES, onStopTeam }: Props) {
  useDetailsMenus();
  const navigate = useNavigate();
  const location = useLocation();
  const { team, openAgent, page, agentTab } = useWhere();
  const [seenVersion, setSeenVersion] = useState(0);
  const data = useShellData(team, openStream, givenLayouts, givenNames, openAgent, seenVersion);
  const now = useNow();
  const [selected, setSelected] = useState<string | undefined>(openAgent);
  const [goto, setGoto] = useState<Scope | false>(false);
  const [help, setHelp] = useState(false);
  const [broadcast, setBroadcast] = useState<{ team: string; ids: string[] } | null>(null);
  const [boardView, setBoardView] = useState<View | null>(null);
  const [answerToken, setAnswerToken] = useState(0);
  const [startWarnings, setWarnings] = useState<Record<string, DraftIssue[]>>({});
  const pending = useRef<Pending | null>(null);
  const lastBoard = useRef<Record<string, string>>({});
  const refs = useShellRefs();
  const { picked, pick, setPicked } = usePicks(team, data.byId);
  const toasts = useToasts(data.rows, openAgent, page === 'board' ? team : undefined);
  const view = viewOf(location.pathname, page, openAgent, boardView);

  useEffect(() => { if (openAgent) setSelected(openAgent); }, [openAgent]);
  // Viewing an agent clears its "done": remember the latest turn end you have seen.
  const openTurnEnd = openAgent ? (data.activity[openAgent]?.lastTurnEndSeq ?? null) : null;
  useEffect(() => { if (openAgent && markSeen(openAgent, openTurnEnd)) setSeenVersion((v) => v + 1); }, [openAgent, openTurnEnd]);
  // The board you leave is the board you come back to (its mode; the viewport and selection are kept by the board).
  useEffect(() => { if (team && page === 'board') lastBoard.current[team] = location.pathname + location.search; }, [team, page, location]);

  const open = useCallback((a: { rig: string; id: string; href?: string }) => { setSelected(a.id); navigate(a.href ?? agentPath(a.rig, a.id)); }, [navigate]);
  const back = useCallback(() => { if (team) navigate(lastBoard.current[team] ?? boardPath(team)); }, [team, navigate]);
  const answerNext = useRef(false);
  const answer = useCallback((rig: string, id: string) => { answerNext.current = true; setAnswerToken((n) => n + 1); open({ rig, id }); }, [open]);
  const isTeam = (id: string) => !!data.teamList.summaries?.some((s) => s.id === id) || !!data.drafts[id];

  const shell: ShellValue = {
    rows: data.rows, byId: data.byId, teams: data.teams, rigs: data.rigs, listError: data.listError, summaries: data.teamList.summaries,
    drafts: data.drafts, titles: data.titles, names: data.names, layouts: data.layouts, now, selected, setSelected, open,
    palette: setGoto, broadcast: (t, ids) => setBroadcast({ team: t, ids }), picked, pick, setPicked, answerToken, answerNext, answer,
    startWarnings, setStartWarnings: (t, w) => setWarnings((cur) => ({ ...cur, [t]: w })), onDraft: data.onDraft, onStopTeam,
    reloadTeams: () => void data.teamList.reload(), isTeam, pending, refs, back, setBoardView,
  };
  const commands = useShellCommands({ shell, view, team, openAgent, agentTab, navigate, setHelp, setGoto, back });
  useShellKeys({ shell, view, team, openAgent, blocked: !!goto || help || !!broadcast, navigate, setGoto, setHelp, back });

  return (
    <ShellContext.Provider value={shell}>
      <div className={`app app-frame view-${view}`} data-path={location.pathname}>
        <TopBar team={team} rows={data.rows} onCommand={() => setGoto('all')} onHelp={() => setHelp(true)} />
        <div className="page-host">
          <Outlet />
        </div>
        <Toasts toasts={toasts.list} onDismiss={toasts.dismiss} onOpen={(t) => { toasts.dismiss(t.id); open({ rig: t.rig, id: t.agent }); }} />
        {broadcast && (
          <BroadcastDialog agents={data.rows.find((r) => r.rig === broadcast.team)?.agents ?? []} initial={broadcast.ids} team={data.titles[broadcast.team] ?? broadcast.team} onClose={() => setBroadcast(null)} />
        )}
        {goto && (
          <CommandPalette teams={data.rows} commands={commands()} project={team ?? data.rows[0]?.rig} scope={goto} now={now} onClose={() => setGoto(false)}
            onJump={(a) => { setGoto(false); open(a); }} />
        )}
        {help && <HelpOverlay onClose={() => setHelp(false)} onCommand={() => { setHelp(false); setGoto('all'); }} />}
        <AppearanceControl trigger={false} />
      </div>
    </ShellContext.Provider>
  );
}

/** A clock for "2m ago" labels, ticking every 15 seconds. */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);
  return now;
}

type KeyDeps = {
  shell: ShellValue;
  view: View;
  team: string | undefined;
  openAgent: string | undefined;
  blocked: boolean;
  navigate: ReturnType<typeof useNavigate>;
  setGoto: (s: Scope | false) => void;
  setHelp: (b: boolean) => void;
  back: () => void;
};

/** Global keys. Bare letters never fire while typing; overlays handle their own keys. */
function useShellKeys(d: KeyDeps) {
  const latest = useRef(d);
  latest.current = d;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const c = latest.current;
      if (e.defaultPrevented || c.blocked) return;
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        c.setGoto('all');
        return;
      }
      if (e.key === 'Escape') return escapeKey(e, c);
      if (isTextField(e.target) || e.altKey || e.ctrlKey || e.metaKey) return;
      letterKey(e, c);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

function escapeKey(e: KeyboardEvent, c: KeyDeps) {
  if (isTextField(e.target)) {
    (e.target as HTMLElement).blur();
    return;
  }
  if (c.view === 'team' && c.shell.picked.length > 0) {
    e.preventDefault();
    c.shell.setPicked([]);
  } else if (c.view === 'agent') {
    e.preventDefault();
    c.back();
  }
}

function letterKey(e: KeyboardEvent, c: KeyDeps) {
  const { refs } = c.shell;
  const onControl = !!(e.target as HTMLElement).closest?.('button, a, summary, [role="option"], [role="separator"]');
  const board = c.view === 'structure' ? refs.board.current : null;
  const graph = c.view === 'team' ? refs.graph.current : null;
  const act = (fn: () => void) => { e.preventDefault(); fn(); };
  switch (e.key) {
    case 'g': case '/': return act(() => c.setGoto('agents'));
    case '?': return act(() => c.setHelp(true));
    case 'n': return act(() => (board?.status() ? board.addKey() : c.navigate('/new')));
    case 'l': if (c.team && (board || c.view === 'team')) act(() => c.navigate(agentsPath(c.team!))); return;
    case 'f': board?.fit(); graph?.fit(); return;
    case '+': case '=': board?.zoomBy(1.2); graph?.zoomBy(1.2); return;
    case '-': board?.zoomBy(1 / 1.2); graph?.zoomBy(1 / 1.2); return;
    case 'i': if (c.view === 'agent') void refs.controls.current?.interrupt(); return;
    case '[': case ']': if (c.view === 'agent') act(() => cycleAgent(c, e.key === ']' ? 1 : -1)); return;
    case 'Enter': {
      const a = c.view === 'team' && !onControl && c.shell.selected ? c.shell.byId[c.shell.selected] : undefined;
      if (a) act(() => c.shell.open(a));
    }
  }
}
