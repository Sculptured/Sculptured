import { createContext, useContext, type RefObject } from 'react';
import type { DraftIssue } from '../generated/DraftIssue';
import type { RigSummary } from '../generated/RigSummary';
import type { StopRigOutcome } from '../generated/StopRigOutcome';
import type { TeamDraft } from '../generated/TeamDraft';
import type { TeamSummary } from '../generated/TeamSummary';
import type { View } from '../keys';
import type { Layout } from '../layout';
import type { BoardHandle } from '../structure/StructureBoard';
import type { Teams } from '../useTeams';
import type { NameMap } from '../vocab';
import type { AgentControls } from './AgentControls';
import type { Scope } from './CommandPalette';
import type { GraphHandle } from './LiveGraph';
import type { AgentRow, TeamRow } from './model';

/** What the project bar can do for the palette and for pages: Start and Stop, through their confirmations. */
export type RunHandle = {
  /** Some agent has, or may have, a process. */
  live: boolean;
  startReason: string | null;
  stopReason: string | null;
  askStart: () => void;
  askStop: () => void;
};

/** An action asked for away from its project (the Projects list, the palette): the project bar runs it once it has loaded. */
export type Pending = { team: string; action: 'start' | 'stop'; until: number };

/** Everything the pages share: the teams and their agents, the open agent, and the app's overlays. */
export type Shell = {
  rows: TeamRow[];
  byId: Record<string, AgentRow>;
  teams: Teams;
  rigs: RigSummary[] | null;
  listError: string | null;
  summaries: TeamSummary[] | null;
  drafts: Record<string, TeamDraft>;
  titles: Record<string, string>;
  names: Record<string, NameMap>;
  layouts: Record<string, Layout>;
  now: number;
  /** The agent selected on a board or list (kept while you open it and come back). */
  selected: string | undefined;
  setSelected: (id: string | undefined) => void;
  open: (a: { rig: string; id: string; href?: string }) => void;
  palette: (scope: Scope) => void;
  broadcast: (team: string, ids: string[]) => void;
  /** Agents picked (Shift+click) on the open team's Run board or Agents list. */
  picked: string[];
  pick: (id: string) => void;
  setPicked: (ids: string[]) => void;
  answerToken: number;
  /** Set by Answer: the agent's message box takes the focus once its conversation shows. */
  answerNext: RefObject<boolean>;
  answer: (rig: string, id: string) => void;
  startWarnings: Record<string, DraftIssue[]>;
  setStartWarnings: (team: string, w: DraftIssue[]) => void;
  onDraft: (d: TeamDraft) => void;
  onStopTeam?: (rig: string, requestId: string) => Promise<StopRigOutcome>;
  reloadTeams: () => void;
  isTeam: (id: string) => boolean;
  pending: RefObject<Pending | null>;
  /** Back to the board this project was last shown on (its mode, viewport and selection). */
  back: () => void;
  /** The board says which mode it shows, for the palette and the keys. */
  setBoardView: (v: View | null) => void;
  refs: {
    board: RefObject<BoardHandle | null>;
    run: RefObject<RunHandle | null>;
    graph: RefObject<GraphHandle | null>;
    controls: RefObject<AgentControls | null>;
    composer: RefObject<HTMLTextAreaElement | null>;
  };
};

export const ShellContext = createContext<Shell | null>(null);

export function useShell(): Shell {
  const s = useContext(ShellContext);
  if (!s) throw new Error('useShell outside the app shell');
  return s;
}
