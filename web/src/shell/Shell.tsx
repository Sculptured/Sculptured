import { track } from '../hosted/analytics';
import { hasProcess } from '../status';
import {DismissibleError} from './DismissibleError';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, Navigate, useLocation, useMatch, useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { RigHome } from '../components/RigHome';
import { isTextField, trackPointer } from '../dom';
import { emptyCounts, STATES, type Counts } from '../herdr';
import type { View } from '../keys';
import type { Layout } from '../layout';
import { markSeen } from '../seen';
import type { OpenStream } from '../useRigStream';
import { useActivity, useTeams } from '../useTeams';
import { nameForId, WORDS, type NameMap } from '../vocab';
import type { StopRigOutcome } from '../generated/StopRigOutcome';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import { api } from '../api';
import { draftLayout } from '../draft/geometry';
import { nameMap } from '../draft/ids';
import { StructureBoard, type BoardHandle } from '../structure/StructureBoard';
import { TemplateGallery } from '../structure/TemplateGallery';
import { useHome } from '../structure/FolderPicker';
import { specStartConfirmation, startConfirmation } from '../structure/StartConfirm';
import { useTeamList } from '../useTeamList';
import type { AgentControls } from './AgentControls';
import { AgentPane } from './AgentPane';
import { GotoPalette } from './GotoPalette';
import { HelpOverlay } from './HelpOverlay';
import type { GraphHandle } from './LiveGraph';
import { agentPath, buildTeams, draftTeamRow, firstMessageIsWork, flatAgents, sortTeams, structurePath, teamPath, type AgentRow } from './model';
import { Sidebar } from './Sidebar';
import { StatusLine } from './StatusLine';
import { ConnectingPanel } from './ConnectingPanel';
import { TeamLive, type Mode } from './TeamLive';
import { DeploymentHistoryView } from '../collaboration/DeploymentHistoryView';
import { Toasts, type Toast } from './Toasts';
import { Btn, Icon, Panel } from './ui';
import { WorkspaceBar } from './WorkspaceBar';
import { LibraryPage, librarySection } from './LibraryPage';
import { ContextPanel } from './ContextPanel';

const NARROW = '(max-width: 720px)';
export const LEFT_PANEL_KEY = 'sculptured.panels.v3.left';
/** Run's List/Graph choice, the same for every team. */
export const RUN_VIEW_KEY = 'sculptured.run.view.v1';
export const RIGHT_PANEL_KEY = 'sculptured.panels.v2.right';
const SIDEBAR_KEY = 'sculptured.sidebar.v1';
/** The project (team) last open in this browser: the sidebar shows its agents. */
export const PROJECT_KEY = 'sculptured.project.v1';
const SIDEBAR_MIN = 220;
const SIDEBAR_MAX = 520;
const SIDEBAR_DEFAULT = 288;

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = globalThis.localStorage?.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}
function writeJson(key: string, value: unknown) {
  try {
    globalThis.localStorage?.setItem(key, JSON.stringify(value));
  } catch {
    // Per-browser convenience only.
  }
}

function useNarrow(): boolean {
  const query = () => typeof window !== 'undefined' && !!window.matchMedia?.(NARROW).matches;
  const [narrow, setNarrow] = useState(query);
  useEffect(() => {
    const mq = window.matchMedia?.(NARROW);
    if (!mq) return;
    const on = () => setNarrow(mq.matches);
    mq.addEventListener?.('change', on);
    return () => mq.removeEventListener?.('change', on);
  }, []);
  return narrow;
}

type Props = {
  openStream?: OpenStream;
  /** Persisted positions per team (Structure board, Slice 2). */
  layouts?: Record<string, Layout>;
  /** Names from team drafts; they win over spec labels. */
  names?: Record<string, NameMap>;
  onStopTeam?: (rig: string, requestId: string) => Promise<StopRigOutcome>;
};

/** The herdr-style shell: status line, agent sidebar, board or agent pane. */
/** Lifecycles in which an agent may have a process (the server's `is_live`). */

export function Shell({ openStream, layouts: givenLayouts = {}, names: givenNames = {}, onStopTeam }: Props) {
  // `/team/:team/structure` and `/team/:team/progress` are the two pages of a team built here.
  const { rig, seat: openAgent, tab, team: structureTeam, collection } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const [params] = useSearchParams();
  const narrow = useNarrow();
  const library = !!useMatch('/library/:collection');
  const collectionId = librarySection(collection);
  const [leftOpen, setLeftOpen] = useState(() => readJson<boolean>(LEFT_PANEL_KEY, true) !== false);
  const [rightOpen, setRightOpen] = useState(() => readJson<boolean>(RIGHT_PANEL_KEY, false) === true);
  const [menuOpen, setMenuOpen] = useState(false);
  const [mobileLeft, setMobileLeft] = useState<boolean | null>(null);
  const [mobileRight, setMobileRight] = useState(false);
  const homeList = narrow && !rig && !structureTeam && !library && !params.has('all');
  const projectsShown = narrow ? (mobileLeft ?? homeList) : leftOpen;
  const detailsShown = narrow ? mobileRight : rightOpen;
  const [workspaceHref, setWorkspaceHref] = useState('/?all');
  useEffect(() => {
    setMenuOpen(false);
    setMobileLeft(null);
    setMobileRight(false);
    if (!library) setWorkspaceHref(location.pathname + location.search);
  }, [location.pathname, location.search, library]);
  const toggleProjects = () => {
    if (narrow) { setMobileLeft(!projectsShown); setMobileRight(false); }
    else { setLeftOpen(!leftOpen); writeJson(LEFT_PANEL_KEY, !leftOpen); }
  };
  const toggleDetails = () => {
    if (narrow) { setMobileRight(!mobileRight); setMobileLeft(false); }
    else { setRightOpen(!rightOpen); writeJson(RIGHT_PANEL_KEY, !rightOpen); }
  };
  const teamList = useTeamList();
  const startedTeams = useMemo(() => (teamList.summaries ?? []).filter((s) => s.started).map((s) => s.id), [teamList.summaries]);
  const { rigs, listError, teams, names: order } = useTeams(rig, openStream, startedTeams);
  // The open Structure board's draft wins over the last one read from the list.
  const [boardDrafts, setBoardDrafts] = useState<Record<string, TeamDraft>>({});
  const onBoardDraft = useCallback((d: TeamDraft) => setBoardDrafts((cur) => (cur[d.id] === d ? cur : { ...cur, [d.id]: d })), []);
  // The team whose Structure board could not load: none of the board's keys apply there.
  const [brokenBoard, setBrokenBoard] = useState<string | null>(null);
  const onBoardLoadError = useCallback(
    (reason: string | null) =>
      setBrokenBoard((cur) => (reason !== null ? (structureTeam ?? null) : cur === structureTeam ? null : cur)),
    [structureTeam],
  );
  const drafts = useMemo(() => ({ ...teamList.drafts, ...boardDrafts }), [teamList.drafts, boardDrafts]);
  const [gallery, setGallery] = useState(false);
  const [startWarnings, setStartWarnings] = useState<Record<string, DraftIssue[]>>({});
  const board = useRef<BoardHandle | null>(null);
  // Teams built here use their draft's names and positions on the Live board too.
  const draftNames = useMemo(() => {
    const out: Record<string, NameMap> = { ...givenNames };
    for (const [id, d] of Object.entries(drafts)) out[id] = { ...nameMap(d), ...(givenNames[id] ?? {}) };
    return out;
  }, [drafts, givenNames]);
  const layouts = useMemo(() => {
    const out: Record<string, Layout> = {};
    for (const [id, d] of Object.entries(drafts)) out[id] = draftLayout(d);
    return { ...out, ...givenLayouts };
  }, [drafts, givenLayouts]);
  const titles = useMemo(() => Object.fromEntries(Object.entries(drafts).map(([id, d]) => [id, d.name.trim() || id])), [drafts]);
  const activity = useActivity(teams);
  const [seenVersion, setSeenVersion] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const [selected, setSelected] = useState<string | undefined>(openAgent);
  const [runView, setRunView] = useState<Mode>(() => (readJson<Mode>(RUN_VIEW_KEY, 'list') === 'graph' ? 'graph' : 'list'));
  const [goto, setGoto] = useState(false);
  const [help, setHelp] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [lastProject, setLastProject] = useState<string | null>(() => readJson<string | null>(PROJECT_KEY, null));
  const [sideW, setSideW] = useState<number>(() => readJson<number>(`${SIDEBAR_KEY}.width`, SIDEBAR_DEFAULT));
  const graph = useRef<GraphHandle | null>(null);
  const controls = useRef<AgentControls | null>(null);
  const composer = useRef<HTMLTextAreaElement | null>(null);
  const answerNext = useRef(false);
  const [answerToken, setAnswerToken] = useState(0);
  const toastId = useRef(0);

  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);

  // The open agent is selected, so Esc returns to the board with it selected.
  useEffect(() => {
    if (openAgent) setSelected(openAgent);
  }, [openAgent]);

  const rows = useMemo(
    () => {
      const built = buildTeams(teams, order, activity, draftNames, titles);
      // A team started here runs its agents without a folder in the home folder ("." from home).
      for (const t of built) if (drafts[t.rig]) for (const a of t.agents) if (a.folder === '.') a.folder = '~';
      // A first agent with no Job: your next message is its work (shown above its message box).
      for (const t of built) {
        const live = teams[t.rig]?.state;
        if (!live?.snapshot) continue;
        for (const a of t.agents) a.firstMessageIsWork = !!a.teamRunning && firstMessageIsWork(live.snapshot.rig, live.seats, drafts[t.rig], a.id);
      }
      // Teams built here that have never started: their agents come from the draft.
      for (const s of teamList.summaries ?? []) {
        if (order.includes(s.id) || s.started) continue;
        const d = drafts[s.id];
        if (d) built.push(draftTeamRow(d));
      }
      // The agent you are looking at has nothing unseen.
      for (const t of built)
        for (const a of t.agents)
          if (a.id === openAgent && a.state.key === 'done') a.state = { ...a.state, ...STATES.idle };
      return sortTeams(built, rigs);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [teams, order, activity, draftNames, titles, teamList.summaries, drafts, rigs, seenVersion, openAgent],
  );

  const byId = useMemo(() => {
    const m: Record<string, AgentRow> = {};
    for (const t of rows) for (const a of t.agents) m[a.id] = a;
    return m;
  }, [rows]);

  const counts = useMemo(() => {
    const c: Counts = emptyCounts();
    for (const t of rows) for (const k of Object.keys(c) as (keyof Counts)[]) c[k] += t.counts[k];
    return c;
  }, [rows]);

  // Viewing an agent clears its "done": remember the latest turn end you have seen.
  const openTurnEnd = openAgent ? (activity[openAgent]?.lastTurnEndSeq ?? null) : null;
  useEffect(() => {
    if (openAgent && markSeen(openAgent, openTurnEnd)) setSeenVersion((v) => v + 1);
  }, [openAgent, openTurnEnd]);

  // Toasts when a background agent becomes "needs you" or "done". First sight never toasts.
  const prevStates = useRef(new Map<string, string>());
  useEffect(() => {
    const fresh: Toast[] = [];
    for (const t of rows) {
      if (!t.loaded) continue;
      for (const a of t.agents) {
        const prev = prevStates.current.get(a.id);
        prevStates.current.set(a.id, a.state.key);
        if (prev === undefined || prev === a.state.key || a.id === openAgent) continue;
        // A turn that ends on the board you are watching is already in front of you.
        if (a.state.key === 'done' && !openAgent && !structureTeam && a.rig === rig) continue;
        // "done" means a turn just ended; idle becoming done is only history arriving.
        if (a.state.key === 'needs-you' || (a.state.key === 'done' && prev === 'working'))
          fresh.push({ id: ++toastId.current, agent: a.id, rig: a.rig, name: a.name, kind: a.state.key, team: t.title });
      }
    }
    if (fresh.length > 0)
      setToasts((cur) => [...cur.filter((x) => !fresh.some((f) => f.agent === x.agent)), ...fresh].slice(-5));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows, openAgent]);

  const dismiss = useCallback((id: number) => setToasts((cur) => cur.filter((t) => t.id !== id)), []);

  const team = rig ? teams[rig] : undefined;
  const snap = team?.state.snapshot ?? null;
  const teamNames = useMemo(
    () => (rig ? (rows.find((r) => r.rig === rig)?.names ?? {}) : {}),
    [rows, rig],
  );
  const mode: Mode = runView;
  const setMode = (m: Mode) => {
    setRunView(m);
    writeJson(RUN_VIEW_KEY, m);
  };
  const view: View = structureTeam ? 'structure' : !rig ? 'home' : openAgent ? 'agent' : 'team';
  const boardBroken = view === 'structure' && brokenBoard === structureTeam;
  const isTeam = (id: string) => !!teamList.summaries?.some((s) => s.id === id) || !!drafts[id];

  const open = useCallback(
    (a: { rig: string; id: string; href?: string }) => {
      setSelected(a.id);
      navigate(a.href ?? agentPath(a.rig, a.id));
    },
    [navigate],
  );
  const created = (id: string, open?: { history?: boolean }) => {
    setGallery(false);
    void teamList.reload();
    navigate(open?.history ? `/rig/${encodeURIComponent(id)}?view=history` : structurePath(id));
  };
  const openInTeam = (id: string) => rig && open({ rig, id });
  const answer = (id: string) => {
    answerNext.current = true;
    setAnswerToken((n) => n + 1);
    openInTeam(id);
  };
  const back = useCallback(() => {
    if (rig) navigate(teamPath(rig));
  }, [rig, navigate]);

  // "Answer" lands in the composer, then in the agent's open question once it has loaded (OpenQuestions).
  useEffect(() => {
    if (openAgent && answerNext.current) {
      answerNext.current = false;
      composer.current?.focus();
    }
  });

  // The current project: the team on screen, else the one last open here, else the most active.
  // Only its agents are listed; every other project is one row you switch to.
  const known = (id: string | null | undefined) => (id && rows.some((r) => r.rig === id) ? id : undefined);
  const routeProject = known(rig ?? structureTeam);
  const project = routeProject ?? known(lastProject) ?? rows[0]?.rig;
  useEffect(() => {
    if (!routeProject) return;
    setLastProject(routeProject);
    writeJson(PROJECT_KEY, routeProject);
  }, [routeProject]);
  const others = useMemo(() => new Set(rows.filter((r) => r.rig !== project).map((r) => r.rig)), [rows, project]);
  const flat = useMemo(() => flatAgents(rows, others), [rows, others]);
  const home = useHome();

  // Global keys. Bare letters never fire while typing; overlays handle their own keys.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || goto || help || gallery) return;
      const field = isTextField(e.target);
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setGoto(true);
        return;
      }
      if (library && e.key !== '?') return;
      if (e.key === 'Escape') {
        if (field) {
          (e.target as HTMLElement).blur();
          return;
        }
        if (openAgent) {
          e.preventDefault();
          back();
        }
        return;
      }
      if (field || e.altKey || e.ctrlKey || e.metaKey) return;
      const target = e.target as HTMLElement;
      const onControl = !!target.closest?.('button, a, summary, [role="option"], [role="separator"]');
      switch (e.key) {
        case 'g':
        case '/':
          e.preventDefault();
          setGoto(true);
          return;
        case '?':
          e.preventDefault();
          setHelp(true);
          return;
        case 'j':
        case 'k': {
          if (flat.length === 0) return;
          e.preventDefault();
          const i = flat.findIndex((a) => a.id === selected);
          const next = e.key === 'j' ? Math.min(flat.length - 1, i + 1) : i < 0 ? 0 : Math.max(0, i - 1);
          setSelected(flat[next].id);
          return;
        }
        case 'Enter': {
          if (onControl) return;
          const a = selected ? byId[selected] : undefined;
          if (a) {
            e.preventDefault();
            open(a);
          }
          return;
        }
        case 'n':
          e.preventDefault();
          if (view === 'structure' && !boardBroken) board.current?.addKey();
          else setGallery(true);
          return;
        case 'l':
          if (view === 'team') setMode(mode === 'graph' ? 'list' : 'graph');
          return;
        case 'f':
          if (view === 'team') graph.current?.fit();
          if (view === 'structure') board.current?.fit();
          return;
        case '+':
        case '=':
          if (view === 'team') graph.current?.zoomBy(1.2);
          if (view === 'structure') board.current?.zoomBy(1.2);
          return;
        case '-':
          if (view === 'team') graph.current?.zoomBy(1 / 1.2);
          if (view === 'structure') board.current?.zoomBy(1 / 1.2);
          return;
        case 'i':
          if (view === 'agent') void controls.current?.interrupt();
          return;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  // Sidebar resize: drag the rule, or focus it and use the arrow keys.
  const resizeStop = useRef<(() => void) | null>(null);
  // Unmounting mid-drag stops listening.
  useEffect(() => () => resizeStop.current?.(), []);
  const startResize = (e: React.PointerEvent) => {
    const start = sideW;
    resizeStop.current?.();
    resizeStop.current = trackPointer(
      e,
      (dx) => setSideW(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, start + dx))),
      () => {
        resizeStop.current = null;
        setSideW((w) => (writeJson(`${SIDEBAR_KEY}.width`, w), w));
      },
    );
  };
  const resizeKey = (e: React.KeyboardEvent) => {
    const d = e.key === 'ArrowLeft' ? -16 : e.key === 'ArrowRight' ? 16 : 0;
    if (!d) return;
    e.preventDefault();
    setSideW((w) => {
      const n = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w + d));
      writeJson(`${SIDEBAR_KEY}.width`, n);
      return n;
    });
  };

  let content: React.ReactNode;
  if (library) {
    content = <LibraryPage section={collectionId} />;
  } else if (structureTeam) {
    const stream = teams[structureTeam];
    const running = stream?.state.snapshot ? Object.values(stream.state.seats).some((s) => hasProcess(s.lifecycle)) : null;
    const summary = teamList.summaries?.find((s) => s.id === structureTeam);
    const started = !!summary?.started || !!rigs?.some((r) => r.name === structureTeam);
    content = (
      <StructureBoard
        key={structureTeam}
        ref={board}
        teamId={structureTeam}
        running={running}
        started={started}
        narrow={narrow}
        detailsShown={detailsShown}
        onShowDetails={() => { setRightOpen(true); writeJson(RIGHT_PANEL_KEY, true); }}
        initialAgent={params.get('agent')}
        onDraft={onBoardDraft}
        onLoadError={onBoardLoadError}
        onStarted={(warnings) => {
          setStartWarnings((cur) => ({ ...cur, [structureTeam]: warnings }));
          void teamList.reload();
          navigate(teamPath(structureTeam));
        }}
      />
    );
  } else if (!rig) {
    if (homeList) content = projectsShown ? null : <Panel title="Workspace"><div className="workspace-empty"><p>Choose a project to continue.</p><Btn onClick={toggleProjects}>Show projects</Btn></div></Panel>;
    else if (params.has('all') || rigs === null || rigs.length > 0) content = <RigHome />;
    else if (teamList.summaries === null && !teamList.error) content = null;
    else if (teamList.summaries && teamList.summaries.length > 0) {
      const newest = [...teamList.summaries].sort((a, b) => b.updated_at_ms - a.updated_at_ms)[0];
      content = <Navigate replace to={structurePath(newest.id)} />;
    } else content = <TemplateGallery inline onCreated={created} />;
  } else if (!snap && params.get('view') === 'history') {
    // History needs no live team: a team that never ran here (an imported run) shows its runs too.
    content = (
      <Panel title={<span className="team-title">{rig}</span>} className="team-live">
        <DeploymentHistoryView rig={rig} />
      </Panel>
    );
  } else if (!snap) {
    content = <ConnectingPanel key={rig} rig={rig} />;
  } else if (openAgent) {
    const a = byId[openAgent];
    if (a?.seat) {
      content = (
        <AgentPane
          key={openAgent}
          a={a}
          tab={tab ?? 'conversation'}
          feed={team!.state.feed[openAgent] ?? []}
          epoch={team!.state.epoch}
          onBack={back}
          controlsRef={controls}
          composerRef={composer}
          answerToken={answerToken}
        />
      );
    } else {
      const inTeam = snap.rig.seats.some((s) => s.id === openAgent);
      content = (
        <Panel title={<span className="team-title">{rig}</span>}>
          <DismissibleError className="pad tone-amber">
            {inTeam ? WORDS.statusUnknown(nameForId(openAgent, teamNames)) : WORDS.notInTeam(nameForId(openAgent, teamNames), rig)}
          </DismissibleError>
          <p className="pad">
            <Link to={teamPath(rig)} className="btn">
              Back to {rig}
            </Link>
          </p>
        </Panel>
      );
    }
  } else {
    const agents: Record<string, AgentRow> = {};
    for (const s of snap.rig.seats) if (byId[s.id]) agents[s.id] = byId[s.id];
    content = (
      <TeamLive
        draft={drafts[rig]}
        rig={rig}
        state={team!.state}
        connected={team!.connected}
        agents={agents}
        names={teamNames}
        layout={layouts[rig]}
        mode={mode}
        onMode={setMode}
        selected={selected}
        onOpen={openInTeam}
        onAnswer={answer}
        onSelect={setSelected}
        graphRef={graph}
        now={now}
        title={titles[rig]}
        teamId={isTeam(rig) ? rig : undefined}
        startConfirm={drafts[rig] ? startConfirmation(drafts[rig], home) : specStartConfirmation(snap.rig, teamNames, home)}
        startWarnings={startWarnings[rig]}
        onDismissWarnings={() => setStartWarnings((cur) => ({ ...cur, [rig]: [] }))}
        onStopTeam={onStopTeam ? (id) => onStopTeam(rig, id) : isTeam(rig) ? (id) => api.stopTeam(rig, id) : undefined}
        onStartTeam={
          isTeam(rig)
            ? async (id, resume, overPlanLimit) => {
                // Start the latest saved version, as the Structure board does.
                const rec = await api.getTeam(rig);
                const r = await api.startTeam(rig, rec.draft.revision, overPlanLimit ? undefined : id, !!overPlanLimit, resume);
                setStartWarnings((cur) => ({ ...cur, [rig]: r.warnings ?? [] }));
                void teamList.reload();
                return r;
              }
            : undefined
        }
      />
    );
  }

  const sidebar = (
    <Sidebar
      teams={rows}
      selected={selected}
      currentAgent={openAgent}
      currentRig={rig ?? structureTeam}
      project={project}
      now={now}
      listError={listError}
      asHome={homeList}
      onNewTeam={() => setGallery(true)}
    />
  );

  return (
    <div className={`app workspace-shell${narrow ? ' narrow' : ''}${projectsShown ? '' : ' projects-hidden'}${library ? ' area-library' : ''} view-${view}`} style={{ ['--sidebar-w' as string]: `${sideW}px` }} data-path={location.pathname}>
      <div className="workspace-header">
      <WorkspaceBar library={library} workspaceHref={workspaceHref} projectsShown={projectsShown} detailsShown={detailsShown} onProjects={toggleProjects} onDetails={toggleDetails}
        team={titles[rig ?? structureTeam ?? ''] ?? rig ?? structureTeam} menuOpen={menuOpen} onMenu={() => setMenuOpen(open => !open)}
        needsAttention={counts['needs-you'] + counts.failed}>
      {(rig||structureTeam)&&<Link className="btn" to={`/team/${rig??structureTeam}/git`} onClick={()=>track('open-git')}>Git changes</Link>}
      <StatusLine
        team={rig ? (titles[rig] ?? rig) : structureTeam ? (titles[structureTeam] ?? structureTeam) : undefined}
        counts={counts}
        now={now}
        onGoto={() => setGoto(true)}
        onHelp={() => setHelp(true)}
        connection={rig && team?.connected ? 'live' : rig && team && !team.connected && team.state.snapshot ? 'reconnecting' : undefined}
      />
      </WorkspaceBar>
      </div>
      <div className={`body${narrow && projectsShown && !homeList ? ' mobile-projects-open' : ''}`}>
        {projectsShown && sidebar}
        {!narrow && projectsShown && (
          <div
            className="resizer"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize agent list"
            aria-valuemin={SIDEBAR_MIN}
            aria-valuemax={SIDEBAR_MAX}
            aria-valuenow={sideW}
            tabIndex={0}
            onPointerDown={startResize}
            onKeyDown={resizeKey}
          />
        )}
        {content && (
          <main className="main" inert={narrow && projectsShown && !homeList}>
            {narrow && (rig || structureTeam) && (
              <Link to="/" className="narrow-back">
                <Icon name="arrow-left" size={14} /> projects <span className="muted">·</span> <span className="team-title">{titles[(rig ?? structureTeam)!] ?? rig ?? structureTeam}</span>
              </Link>
            )}
            <div className="workspace-content">
              <div className="workspace-stage" inert={narrow && detailsShown}>{content}</div>
              {detailsShown && (!structureTeam || narrow) && <ContextPanel library={library ? collectionId : undefined} agent={!library && selected && byId[selected]?.rig === (rig ?? structureTeam) ? byId[selected] : undefined} team={rows.find(r => r.rig === (rig ?? structureTeam))} />}
            </div>
          </main>
        )}
      </div>
      <Toasts
        toasts={toasts}
        onDismiss={dismiss}
        onOpen={(t) => {
          dismiss(t.id);
          open({ rig: t.rig, id: t.agent });
        }}
      />
      {goto && (
        <GotoPalette
          teams={rows}
          now={now}
          onClose={() => setGoto(false)}
          onJump={(a) => {
            setGoto(false);
            open(a);
          }}
        />
      )}
      {help && <HelpOverlay onClose={() => setHelp(false)} />}
      {gallery && <TemplateGallery onCreated={created} onClose={() => setGallery(false)} />}
    </div>
  );
}
