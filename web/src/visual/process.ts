/**
 * A visual construction of an agent's process, derived from its recorded events only. Pure:
 * the same events always give the same process. Nothing is guessed:
 *
 * - phases, steps, files and checks come from protocol events and captured tool calls
 *   (◆ observed);
 * - a check's outcome repeats what its captured result says, never more;
 * - the agent's replies, and the claims in them, stay ◇ reported.
 *
 * Every element carries the key of the transcript item it came from, so the view can jump to
 * that point in the Transcript.
 */
import type { MessageOrigin } from '../conversation';
import type { MessageState } from '../generated/MessageState';
import type { TurnStatus } from '../generated/TurnStatus';
import type { CheckKind, CheckOutcome, EditAction, Hunk, ToolKind } from './tools';

export type StepStatus = 'running' | 'ok' | 'error';

export type Step = {
  key: string;
  callId: string;
  name: string;
  kind: ToolKind;
  summary: string;
  status: StepStatus;
  /** Call to result; null while running. */
  ms: number | null;
  /** Called by a helper agent inside this turn. */
  helper: boolean;
  input: unknown;
  output?: string;
  isError?: boolean;
};

export type Trigger = {
  kind: MessageOrigin;
  key: string;
  firstLine: string;
  /** The whole message, verbatim. */
  text: string;
  /** Upstream agents, for a handoff. */
  from: string[];
  state: MessageState;
  reason?: string;
};

/** `cut-off`: the session ended before the turn did. */
export type PhaseStatus = TurnStatus | 'running' | 'cut-off';

export type Phase = {
  id: string;
  n: number;
  turnId: string | null;
  triggers: Trigger[];
  /** Latest captured job or operator request, preserved through reply turns. */
  goal?: Trigger;
  steps: Step[];
  counts: Partial<Record<ToolKind, number>>;
  failedSteps: number;
  /** The turn's last reply, ◇ reported. */
  reply: { key: string; firstLine: string; text: string; provenance: string } | null;
  /** Public agent messages retained for inspectable explanations. */
  explanations?: {key:string;text:string;provenance:string}[];
  status: PhaseStatus;
  startMs: number;
  endMs: number | null;
  tokens: { input: number; output: number } | null;
  /** The transcript item where this phase begins. */
  anchorKey: string | null;
  endKey: string | null;
};

export type FileChange = { key: string; phase: number; action: EditAction; hunks: Hunk[]; status: StepStatus };
export type FileEntry = {
  path: string;
  reads: number;
  changes: FileChange[];
  last: { action: 'read' | EditAction; phase: number; key: string };
};
export type Check = { key: string; phase: number; command: string; kind: CheckKind; outcome: CheckOutcome; helper: boolean };
/** A sentence in a reply that says the work is done or passing: the agent's word only. */
export type Claim = { key: string; phase: number; text: string };

export type ProcessSummary = {
  turns: number;
  toolCalls: number;
  byKind: Record<ToolKind, number>;
  filesChanged: number;
  filesRead: number;
  commands: number;
  checks: number;
  checksWithError: number;
  failedSteps: number;
  lastActivityMs: number | null;
  sessions: number;
};

export type Process = {
  phases: Phase[];
  files: FileEntry[];
  checks: Check[];
  claims: Claim[];
  /** Messages not read by any turn: still waiting, or never sent. */
  waiting: Trigger[];
  summary: ProcessSummary;
};

export { claimIn, deriveProcess } from './processDerive';

export type TreeNode = { name: string; path: string; children: TreeNode[]; isFile: boolean };

/**
 * Paths as a folder tree under their common folder (`root`). Folders come before files, each
 * sorted by name; a folder with a single sub-folder and no files is folded into one row.
 */
export function fileTree(paths: string[]): { root: string; nodes: TreeNode[] } {
  if (paths.length === 0) return { root: '', nodes: [] };
  const split = paths.map((p) => p.split('/'));
  let common = split[0].slice(0, -1);
  for (const s of split.slice(1)) {
    let i = 0;
    while (i < common.length && i < s.length - 1 && common[i] === s[i]) i++;
    common = common.slice(0, i);
  }
  const root = common.join('/');
  const top: TreeNode = { name: '', path: root, children: [], isFile: false };
  for (const [idx, parts] of split.entries()) {
    let node = top;
    const rest = parts.slice(common.length);
    rest.forEach((name, i) => {
      const isFile = i === rest.length - 1;
      let child = node.children.find((c) => c.name === name && c.isFile === isFile);
      if (!child) {
        child = { name, path: isFile ? paths[idx] : [...common, ...rest.slice(0, i + 1)].join('/'), children: [], isFile };
        node.children.push(child);
      }
      node = child;
    });
  }
  const tidy = (n: TreeNode): TreeNode => {
    let cur = n;
    while (!cur.isFile && cur.children.length === 1 && !cur.children[0].isFile && cur.name) {
      const only = cur.children[0];
      cur = { ...only, name: `${cur.name}/${only.name}` };
    }
    cur.children = cur.children.map(tidy).sort((a, b) => Number(a.isFile) - Number(b.isFile) || a.name.localeCompare(b.name));
    return cur;
  };
  return { root, nodes: tidy(top).children };
}
