import { createContext, useContext } from 'react';
import type { DraftIssue } from '../generated/DraftIssue';
import type { TeamDraft } from '../generated/TeamDraft';
import type { RigState } from '../useRigStream';
import type { TeamDraftApi } from '../useTeamDraft';
import type { TeamRow } from '../shell/model';

/** One project, as every page under `/p/:team` sees it. */
export type Project = {
  team: string;
  title: string;
  /** The design and its autosaving edits (one per open project, so undo survives moving between pages). */
  t: TeamDraftApi;
  /** The design on screen: the saved one, or the board's latest edit. Null while it loads or when the team has none. */
  draft: TeamDraft | null;
  /** Problems with the design that are still on it. */
  issues: DraftIssue[];
  /** Built here (it has a design); a team started from a file has none. */
  builtHere: boolean;
  /** It has been started at least once, so it has agents to watch. */
  started: boolean;
  /** Its live state, once the stream has it. */
  live: RigState | null;
  connected: boolean;
  /** Some agent has, or may have, a process: the design's structure is locked. */
  running: boolean;
  row: TeamRow | undefined;
  /** Why Start must wait for something on screen (the design assistant's proposal); null when nothing does. */
  startBlock: string | null;
  setStartBlock: (why: string | null) => void;
};

export const ProjectContext = createContext<Project | null>(null);

export function useProject(): Project {
  const p = useContext(ProjectContext);
  if (!p) throw new Error('useProject outside a project');
  return p;
}
