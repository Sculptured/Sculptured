import { LOOKS, MODES, readAppearance, updateAppearance, type ColorMode, type Look } from '../appearance';
import type { View } from '../keys';
import { agentPagePath, agentsPath, boardPath, DOCS_URL, resultsPath, settingsPath, type AgentTab } from '../paths';
import type { BoardHandle, BoardStatus } from '../structure/StructureBoard';
import type { RunHandle } from './ShellContext';
import { BOARD_STYLES, readBoardStyle, setBoardStyle, type BoardStyle } from './boardStyle';

/** One thing the command palette can do. Every command also has a click target elsewhere. */
export type Command = {
  id: string;
  title: string;
  /** The heading it is listed under. */
  section: string;
  /** The key that does the same thing, shown in its row so the palette teaches it. */
  keys?: string;
  /** Other words it answers to. */
  words?: string;
  /** Why it cannot run now, shown in its row; null or absent when it can. */
  disabled?: string | null;
  /** Listed before you type, lowest first; absent = only found by searching. */
  rank?: number;
  run: () => void;
};

/** The palette's view of the screen: what is open and what can be done there. */
export type CommandContext = {
  view: View;
  team?: { id: string; title: string; builtHere: boolean; started: boolean; running: boolean };
  board?: BoardHandle | null;
  /** The open project's Start and Stop (the project bar's, with its confirmations). */
  run?: RunHandle | null;
  /** Agents picked on the Run board or the Agents list (Shift+click). */
  picked: number;
  /** Agents of the open team that can be sent a message now. */
  runningAgents: number;
  /** The open agent's page. */
  agent?: { id: string; tab: AgentTab; draftOnly: boolean };
  /** Agent view: why Interrupt cannot run now (null = it can). */
  interruptReason?: string | null;
  /** Agent view: Hand over again (absent when the agent hands work to no one). */
  handOver?: { reason: string | null; ask: () => void } | null;
  /** The other projects, to switch to. */
  projects: { id: string; title: string; href: string }[];
  go: (to: string) => void;
  act: {
    help: () => void;
    appearance: () => void;
    fit: () => void;
    messageAll: () => void;
    messageSelected: () => void;
    interrupt: () => void;
    cycleAgent: (step: 1 | -1) => void;
    back: () => void;
  };
};

type Team = NonNullable<CommandContext['team']>;
const some = (xs: (Command | false | null | undefined)[]): Command[] => xs.filter((x): x is Command => !!x);

/** Every command for the current screen. Commands that mean nothing here are left out; ones that mean something but cannot run say why. */
export function buildCommands(c: CommandContext): Command[] {
  const status = c.view === 'structure' ? (c.board?.status() ?? null) : null;
  return [
    ...(c.team ? [...startStop(c), ...messaging(c, c.team), ...teamPages(c, c.team)] : []),
    ...(c.view === 'structure' ? boardCommands(c, status) : []),
    ...(c.view === 'team' ? runCommands(c) : []),
    ...(c.view === 'structure' || c.view === 'team' ? boardStyleCommands() : []),
    ...(c.view === 'agent' && c.team ? agentCommands(c, c.team) : []),
    ...projectCommands(c),
    ...appCommands(c),
  ];
}

/** Start and Stop, each through the confirmation its button uses. */
function startStop(c: CommandContext): Command[] {
  const run = c.run;
  if (!run) return [];
  return some([
    !run.live && { id: 'team.start', section: 'Team', title: 'Start team…', words: 'run launch go', rank: 1, disabled: run.startReason, run: run.askStart },
    { id: 'team.stop', section: 'Team', title: 'Stop team…', words: 'halt end kill', rank: run.live ? 5 : undefined, disabled: run.stopReason, run: run.askStop },
  ]);
}

/** One message to every running agent, or to the agents picked on the Run board or the Agents list. */
function messaging(c: CommandContext, t: Team): Command[] {
  const none = c.runningAgents === 0 ? (t.started ? 'No agent is running.' : 'Start the team first.') : null;
  return some([
    { id: 'team.message-all', section: 'Team', title: 'Message all agents…', words: 'broadcast send tell everyone', rank: none ? undefined : 3, disabled: none, run: c.act.messageAll },
    (c.view === 'team' || c.view === 'agents') && {
      id: 'team.message-selected',
      section: 'Team',
      title: 'Message selected agents…',
      words: 'broadcast send picked',
      rank: c.picked > 0 ? 2 : undefined,
      disabled: none ?? (c.picked === 0 ? 'Select agents first: Shift+click them.' : null),
      run: c.act.messageSelected,
    },
  ]);
}

/** The project's pages: Board (Design or Run), Agents, Results, Settings, and its history and Git changes. */
function teamPages(c: CommandContext, t: Team): Command[] {
  const notStarted = t.started ? null : 'Run opens once the team has started.';
  return some([
    t.builtHere && c.view !== 'structure' && { id: 'view.design', section: 'Team', title: 'Open the Board: Design', words: 'edit structure', rank: 8, run: () => c.go(boardPath(t.id, 'design')) },
    c.view !== 'team' && { id: 'view.run', section: 'Team', title: 'Open the Board: Run', words: 'live progress watch graph', rank: t.started ? 7 : undefined, disabled: notStarted, run: () => c.go(boardPath(t.id, 'run')) },
    c.view !== 'agents' && { id: 'view.agents', section: 'Team', title: 'Open Agents', words: 'list table everyone', rank: 9, run: () => c.go(agentsPath(t.id)) },
    c.view !== 'results' && { id: 'view.results', section: 'Team', title: 'Open Results', words: 'summary files checks finished outcome', rank: t.started ? 9 : undefined, run: () => c.go(resultsPath(t.id)) },
    c.view !== 'settings' && { id: 'view.settings', section: 'Team', title: 'Open Settings', words: 'team name models budget export delete trash', run: () => c.go(settingsPath(t.id)) },
    { id: 'team.history', section: 'Team', title: 'Open run history', words: 'past runs', disabled: notStarted, run: () => c.go(resultsPath(t.id, 'history')) },
    { id: 'team.git', section: 'Team', title: 'Open Git changes', words: 'diff commit files', run: () => c.go(`/team/${encodeURIComponent(t.id)}/git`) },
  ]);
}

function boardCommands(c: CommandContext, status: BoardStatus | null): Command[] {
  const lock = status?.lockReason ?? (status ? null : 'Loading…');
  const section = 'Design board';
  return [
    { id: 'board.add-claude', section, title: 'Add a Claude agent', keys: 'n  ⏎', words: 'new agent', rank: 2, disabled: lock, run: () => c.board?.add('claude') },
    { id: 'board.add-codex', section, title: 'Add a Codex agent', words: 'new agent', rank: 3, disabled: lock, run: () => c.board?.add('codex') },
    { id: 'board.add-group', section, title: 'Add a group', words: 'new box', disabled: lock, run: () => c.board?.addGroup() },
    { id: 'board.select-all', section, title: 'Select all agents', keys: 'Ctrl+A', words: 'multi bulk every', rank: 4, run: () => c.board?.selectAll() },
    { id: 'board.undo', section, title: 'Undo', keys: 'Ctrl+Z', disabled: status?.canUndo ? null : 'Nothing to undo.', run: () => c.board?.undo() },
    { id: 'board.redo', section, title: 'Redo', keys: 'Ctrl+Shift+Z', disabled: status?.canRedo ? null : 'Nothing to redo.', run: () => c.board?.redo() },
    { id: 'board.fit', section, title: 'Fit the board to the view', keys: 'f', words: 'zoom', run: c.act.fit },
    ...(status?.workflow ? [{ id: 'board.workflow-list', section, title: 'Open the workflow list', words: 'stages items advanced', run: () => c.board?.workflowList() }] : []),
  ];
}

function runCommands(c: CommandContext): Command[] {
  return [{ id: 'run.fit', section: 'Run board', title: 'Fit the graph to the view', keys: 'f', words: 'zoom', run: c.act.fit }];
}

/** Board style: Cards or Logos, on both boards. The one in use says so instead of running. */
function boardStyleCommands(): Command[] {
  const now = readBoardStyle();
  return (Object.keys(BOARD_STYLES) as BoardStyle[]).map((style) => ({
    id: `board.style.${style}`,
    section: 'Board',
    title: `Board style: ${BOARD_STYLES[style]}`,
    words: style === 'logos' ? 'logos icons fun animated provider claude codex blocks' : 'cards blocks details boxes',
    disabled: now === style ? 'In use' : null,
    run: () => setBoardStyle(style),
  }));
}

function agentCommands(c: CommandContext, t: Team): Command[] {
  const a = c.agent;
  const tab = (id: AgentTab, title: string, words: string, rank?: number): Command | false =>
    !!a && a.tab !== id && { id: `agent.${id}`, section: 'Agent', title, words, rank, run: () => c.go(agentPagePath(t.id, a.id, id)) };
  return some([
    tab('setup', 'Open agent setup', 'edit job folder name model settings', 4),
    tab('conversation', 'Open the conversation', 'transcript chat message', 4),
    tab('activity', 'Open activity and evidence', 'progress raw events files visual', undefined),
    !a?.draftOnly && { id: 'agent.interrupt', section: 'Agent', title: 'Interrupt the current turn', keys: 'i', words: 'pause stop turn', rank: 2, disabled: c.interruptReason ?? null, run: c.act.interrupt },
    !!c.handOver && { id: 'agent.hand-over', section: 'Agent', title: 'Hand over again…', words: 'handoff rerun resend downstream pass on', rank: 3, disabled: c.handOver.reason, run: c.handOver.ask },
    { id: 'agent.next', section: 'Agent', title: 'Next agent', keys: ']', words: 'cycle', rank: 5, run: () => c.act.cycleAgent(1) },
    { id: 'agent.prev', section: 'Agent', title: 'Previous agent', keys: '[', words: 'cycle', run: () => c.act.cycleAgent(-1) },
    { id: 'agent.back', section: 'Agent', title: 'Back to the board', keys: 'Esc', run: c.act.back },
  ]);
}

function projectCommands(c: CommandContext): Command[] {
  return [
    { id: 'app.new-project', section: 'Projects', title: 'New project…', keys: 'n', words: 'create team template', rank: c.view === 'home' ? 1 : 9, run: () => c.go('/new') },
    ...c.projects.map((p) => ({ id: `project.${p.id}`, section: 'Projects', title: `Switch to ${p.title}`, words: 'project team open', run: () => c.go(p.href) })),
    { id: 'app.all-teams', section: 'Projects', title: 'All projects', words: 'home overview teams list', run: () => c.go('/') },
  ];
}

/** What sits in the top bar and its ⋯ app menu. */
function appCommands(c: CommandContext): Command[] {
  return [
    { id: 'app.library', section: 'Go to', title: 'Library', words: 'saved groups templates', run: () => c.go('/library/groups') },
    { id: 'app.community', section: 'Go to', title: 'Community', words: 'share profile', run: () => c.go('/community') },
    { id: 'community.people', section: 'Go to', title: 'Community: People', words: 'profiles directory', run: () => c.go('/community/people') },
    { id: 'community.me', section: 'Go to', title: 'Your Community profile', words: 'handle avatar bio', run: () => c.go('/community/me') },
    { id: 'community.saved', section: 'Go to', title: 'Community: Saved blocks', run: () => c.go('/community/saved') },
    { id: 'community.publish', section: 'Go to', title: 'Publish a group to Community…', words: 'share upload', run: () => c.go('/community/publish') },
    { id: 'app.docs', section: 'Go to', title: 'Docs', words: 'help guides manual documentation how to', run: () => window.open(DOCS_URL, '_blank', 'noopener') },
    { id: 'app.account', section: 'Go to', title: 'Account and usage', words: 'tokens plan sign in settings', run: () => c.go('/account') },
    { id: 'app.appearance', section: 'App', title: 'Appearance…', words: 'font text size theme settings', run: c.act.appearance },
    ...lookCommands(),
    { id: 'app.help', section: 'App', title: 'Keyboard shortcuts', keys: '?', words: 'keys help', rank: 10, run: c.act.help },
  ];
}

/** Switch the look and, for Standard, light or dark. The one in use says so instead of running. */
function lookCommands(): Command[] {
  const now = readAppearance();
  const looks = (Object.keys(LOOKS) as Look[]).map((look): Command => ({
    id: `app.look.${look}`, section: 'App', title: `Switch look: ${LOOKS[look].label}`, words: 'theme appearance style colours',
    disabled: now.look === look ? 'In use' : null, run: () => updateAppearance({ look }),
  }));
  const modes = (Object.keys(MODES) as ColorMode[]).map((mode): Command => ({
    id: `app.mode.${mode}`, section: 'App', title: `Mode: ${MODES[mode]}`, words: 'theme light dark system appearance',
    disabled: now.look !== 'standard' ? 'Standard look only' : now.mode === mode ? 'In use' : null,
    run: () => updateAppearance({ mode }),
  }));
  return [...looks, ...modes];
}

const RECENT_KEY = 'sculptured.palette.recent.v1';
const RECENT_MAX = 8;

/** What you ran last, newest first: `cmd:<id>` and `agent:<id>`. Per browser. */
export function readRecent(): string[] {
  try {
    const v = JSON.parse(globalThis.localStorage?.getItem(RECENT_KEY) ?? '[]');
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

export function pushRecent(key: string) {
  try {
    globalThis.localStorage?.setItem(RECENT_KEY, JSON.stringify([key, ...readRecent().filter((k) => k !== key)].slice(0, RECENT_MAX)));
  } catch {
    // A per-browser convenience only.
  }
}
