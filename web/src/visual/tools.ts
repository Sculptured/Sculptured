/**
 * What a tool call is, read from its name and input only. Pure functions shared by the
 * transcript's tool cards and the Visual progress tab. Nothing here runs or guesses at
 * results: a call's kind and summary come from its recorded input (◆ observed), and a
 * check's outcome only repeats what its captured result says.
 */

export type ToolKind = 'read' | 'edit' | 'bash' | 'search' | 'web' | 'task' | 'other';

export const TOOL_KINDS: ToolKind[] = ['read', 'edit', 'bash', 'search', 'web', 'task', 'other'];

/** Plain words for each kind, singular. */
export const KIND_WORD: Record<ToolKind, string> = {
  read: 'read',
  edit: 'edit',
  bash: 'command',
  search: 'search',
  web: 'web',
  task: 'helper agent',
  other: 'other tool',
};

const KINDS: [ToolKind, RegExp][] = [
  ['edit', /^(edit|multiedit|write|notebookedit|apply_patch|str_replace_editor|str_replace_based_edit_tool|create_file|write_file|edit_file)$/],
  ['read', /^(read|notebookread|read_file|view|view_file|open_file)$/],
  ['bash', /^(bash|bashoutput|killshell|killbash|shell|exec_command|local_shell|run_command|run_terminal_cmd|terminal)$/],
  ['search', /^(grep|glob|ls|list_dir|list_files|find|file_search|codebase_search|search)$/],
  ['web', /^(webfetch|websearch|web_search|web_fetch|fetch)$/],
  ['task', /^(task|agent|dispatch_agent|spawn_agent)$/],
];

export function toolKind(name: string): ToolKind {
  const n = name.toLowerCase();
  for (const [kind, re] of KINDS) if (re.test(n)) return kind;
  return 'other';
}

function obj(input: unknown): Record<string, unknown> {
  return input && typeof input === 'object' && !Array.isArray(input) ? (input as Record<string, unknown>) : {};
}

function str(v: unknown): string | null {
  if (typeof v === 'string' && v.trim()) return v;
  if (Array.isArray(v) && v.every((x) => typeof x === 'string') && v.length) return v.join(' ');
  return null;
}

function firstLine(s: string): string {
  const i = s.indexOf('\n');
  return i < 0 ? s : `${s.slice(0, i).trimEnd()} …`;
}

/** The file a call reads or writes, from its input. */
export function toolPath(input: unknown): string | null {
  const o = obj(input);
  return str(o.file_path) ?? str(o.notebook_path) ?? str(o.path) ?? str(o.filename) ?? null;
}

type Obj = Record<string, unknown>;

const searchSummary = (o: Obj) => {
  const pattern = str(o.pattern) ?? str(o.query);
  const where = str(o.path);
  return pattern ? (where ? `${pattern} in ${where}` : pattern) : where;
};

/** What each kind of call acts on, first choice. */
const PRIMARY: Partial<Record<ToolKind, (o: Obj, name: string, input: unknown) => string | null>> = {
  bash: (o) => str(o.command) ?? str(o.cmd),
  read: (_o, name, input) => pathSummary(name, input),
  edit: (_o, name, input) => pathSummary(name, input),
  search: searchSummary,
  web: (o) => str(o.url) ?? str(o.query),
  task: (o) => str(o.description) ?? str(o.prompt),
};

const pathSummary = (name: string, input: unknown) => toolPath(input) ?? (typeof input === 'string' ? editOf(name, input)?.path ?? null : null);

/** The same input read without knowing the kind: a command, a path, a description, a query, an address. */
const anySummary = (o: Obj, input: unknown) => str(o.command) ?? toolPath(input) ?? str(o.description) ?? str(o.query) ?? str(o.url);

/** The input as text, when nothing in it names what the call acts on. */
const asText = (input: unknown) => (input === null || input === undefined ? '' : typeof input === 'string' ? input : (JSON.stringify(input) ?? ''));

/** One line saying what the call acts on: a path, a command, a pattern, an address. */
export function toolSummary(name: string, input: unknown): string {
  const o = obj(input);
  const s = PRIMARY[toolKind(name)]?.(o, name, input) ?? anySummary(o, input);
  return firstLine(s ?? asText(input));
}

export type Hunk = { old: string; new: string };
/** `created` and `wrote` replace a whole file; `edited` changes part of one. */
export type EditAction = 'edited' | 'wrote' | 'created';
export type EditInfo = { path: string; action: EditAction; hunks: Hunk[] };

/** An `apply_patch` call: the file it adds, updates or deletes, with the patch as its one hunk. */
function patchEdit(input: unknown, o: Obj): EditInfo | null {
  const patch = typeof input === 'string' ? input : (str(o.input) ?? str(o.patch));
  const m = patch ? /^\*\*\* (Add|Update|Delete) File: (.+)$/m.exec(patch) : null;
  if (!patch || !m) return null;
  return { path: m[2].trim(), action: m[1] === 'Add' ? 'created' : 'edited', hunks: [{ old: '', new: patch }] };
}

/** An edit tool's input: replacements, or the whole new content of a file. */
function plainEdit(path: string, o: Obj, output: string | undefined): EditInfo {
  if (Array.isArray(o.edits)) return { path, action: 'edited', hunks: o.edits.map((e) => ({ old: String(obj(e).old_string ?? ''), new: String(obj(e).new_string ?? '') })) };
  if (typeof o.old_string === 'string' || typeof o.new_string === 'string') return { path, action: 'edited', hunks: [{ old: String(o.old_string ?? ''), new: String(o.new_string ?? '') }] };
  const content = typeof o.content === 'string' ? o.content : typeof o.new_source === 'string' ? o.new_source : null;
  if (content === null) return { path, action: 'edited', hunks: [] };
  const created = output !== undefined && /\bcreated\b/i.test(output.split('\n')[0] ?? '');
  return { path, action: created ? 'created' : 'wrote', hunks: [{ old: '', new: content }] };
}

/**
 * The change an edit call asked for, from its input. A Write is `wrote` unless the tool's
 * own result says it created the file. Null for calls that change no file.
 */
export function editOf(name: string, input: unknown, output?: string): EditInfo | null {
  const o = obj(input);
  if (name.toLowerCase() === 'apply_patch') return patchEdit(input, o);
  if (toolKind(name) !== 'edit') return null;
  const path = toolPath(input);
  return path ? plainEdit(path, o, output) : null;
}

export type DiffLine = { op: ' ' | '-' | '+'; text: string };

const lines = (s: string) => (s === '' ? [] : s.split('\n'));

/** A line diff by longest common subsequence; large inputs show as all removed then all added. */
export function lineDiff(a: string, b: string): DiffLine[] {
  const x = lines(a);
  const y = lines(b);
  if (x.length * y.length > 250_000) return [...x.map((t) => ({ op: '-' as const, text: t })), ...y.map((t) => ({ op: '+' as const, text: t }))];
  const n = x.length;
  const m = y.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      out.push({ op: ' ', text: x[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) out.push({ op: '-', text: x[i++] });
    else out.push({ op: '+', text: y[j++] });
  }
  while (i < n) out.push({ op: '-', text: x[i++] });
  while (j < m) out.push({ op: '+', text: y[j++] });
  return out;
}

export type CheckKind = 'test' | 'build' | 'typecheck' | 'lint';

const PM = String.raw`(?:npm|pnpm|yarn|bun)`;
const CHECKS: [CheckKind, RegExp][] = [
  [
    'test',
    new RegExp(
      [
        String.raw`\b${PM}\s+(?:run\s+)?test\b`,
        String.raw`\b(?:vitest|jest|mocha|ava|rspec|phpunit|ctest)\b`,
        String.raw`\bplaywright\s+test\b`,
        String.raw`\bcargo\s+(?:test|nextest)\b`,
        String.raw`\bpytest\b|\bpython3?\s+-m\s+(?:pytest|unittest)\b`,
        String.raw`\bgo\s+test\b`,
        String.raw`\b(?:mvn|gradlew?|\.\/gradlew)\s+(?:test|verify|check)\b`,
        String.raw`\bdotnet\s+test\b`,
        String.raw`\b(?:make|just)\s+(?:test|check)\b`,
      ].join('|'),
    ),
  ],
  ['typecheck', new RegExp(String.raw`\btsc\b|\bmypy\b|\bpyright\b|\b${PM}\s+(?:run\s+)?(?:typecheck|type-check|check-types)\b`)],
  ['lint', new RegExp(String.raw`\beslint\b|\bcargo\s+clippy\b|\bruff\b|\bflake8\b|\bpylint\b|\bgolangci-lint\b|\b${PM}\s+(?:run\s+)?lint\b|\bcargo\s+fmt\b.*--check`)],
  [
    'build',
    new RegExp(
      [
        String.raw`\b${PM}\s+(?:run\s+)?build\b`,
        String.raw`\bcargo\s+(?:build|check)\b`,
        String.raw`\bgo\s+build\b`,
        String.raw`\b(?:vite|next|nuxt)\s+build\b`,
        String.raw`\bdotnet\s+build\b`,
        String.raw`\b(?:mvn|gradlew?)\s+(?:package|build|compile)\b`,
        String.raw`\bmake(?:\s+(?:build|all))?\s*$`,
      ].join('|'),
    ),
  ],
];

/** Whether a shell command runs tests, a build, a type check or a linter, by its words. */
export function checkOf(command: string): CheckKind | null {
  // Text inside quotes (commit messages, echo) is not the command being run.
  const bare = command.replace(/"[^"]*"|'[^']*'/g, '""');
  for (const [kind, re] of CHECKS) if (re.test(bare)) return kind;
  return null;
}

export type CheckOutcome = {
  /** `error`: the tool result was flagged as an error. `no-error`: it finished without that flag. */
  status: 'running' | 'error' | 'no-error';
  /** An exit code the output states, if any. */
  exit: number | null;
  /** The output's own summary line, verbatim, if one looks like a result. */
  line: string | null;
  /** The output mentions failures although the tool did not flag an error. */
  mentionsFailure: boolean;
};

const SUMMARY = /\b(passed|failed|passing|failing|test result|tests?:|errors?\b|warnings?\b|ok\b|success|succeeded|finished|built|compiled)/i;
const FAILURE = /\b([1-9]\d*)\s+(?:failed|failing|failures?|errors?)\b|\bFAIL(?:ED)?\b|\berror(?:\[|:)/;

/** What a check's captured result says, and nothing more. */
export function checkOutcome(output: string | undefined, isError: boolean | undefined): CheckOutcome {
  if (output === undefined) return { status: 'running', exit: null, line: null, mentionsFailure: false };
  const exitM = /exit(?:ed with)? code[:\s]+(-?\d+)/i.exec(output);
  const ls = output.split('\n').map((l) => l.trim()).filter(Boolean);
  let line: string | null = null;
  for (let i = ls.length - 1; i >= 0; i--) {
    if (/^exit code/i.test(ls[i])) continue;
    if (SUMMARY.test(ls[i])) {
      line = ls[i].length > 160 ? `${ls[i].slice(0, 160)}…` : ls[i];
      break;
    }
  }
  return {
    status: isError ? 'error' : 'no-error',
    exit: exitM ? Number(exitM[1]) : null,
    line,
    mentionsFailure: !isError && FAILURE.test(output),
  };
}
