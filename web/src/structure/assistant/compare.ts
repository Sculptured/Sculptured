import type { DraftAgent } from '../../generated/DraftAgent';
import type { DraftConnection } from '../../generated/DraftConnection';
import type { DraftGroup } from '../../generated/DraftGroup';
import type { TeamDraft } from '../../generated/TeamDraft';

export type Change = 'added' | 'changed' | 'removed';
/** What each card, group and arrow is in a proposal, by id; unchanged parts have no entry. */
export type Marks = Record<string, Change>;

export type ChangeRow = {
  change: Change;
  /** The part on the board to open, if it is drawn. */
  target: { kind: 'agent' | 'group' | 'connection'; id: string } | null;
  what: string;
  /** For a change: which fields differ. */
  details: string[];
  /** For a changed agent: the saved and proposed versions, to compare its job. */
  before?: DraftAgent;
  after?: DraftAgent;
};

export type Comparison = {
  marks: Marks;
  rows: ChangeRow[];
  /** The proposal, plus the removed parts (drawn faded) so they can be seen where they were. */
  shown: TeamDraft;
};

export const markClass = (marks: Marks | undefined, id: string) => (marks?.[id] ? ` change-${marks[id]}` : '');

const sameEnds = (a: DraftConnection, b: DraftConnection) => a.from === b.from && a.to === b.to && a.kind === b.kind;

function agentDetails(was: DraftAgent, now: DraftAgent, groupName: (id: string | null) => string): string[] {
  const out: string[] = [];
  if (was.name !== now.name) out.push(`name: ${was.name} → ${now.name}`);
  if (was.kind !== now.kind) out.push(`kind: ${was.kind} → ${now.kind}`);
  if (was.job !== now.job) out.push('job');
  if (was.advanced.model !== now.advanced.model) out.push(`model: ${was.advanced.model ?? 'default'} → ${now.advanced.model ?? 'default'}`);
  if (was.group_id !== now.group_id) out.push(`group: ${groupName(was.group_id)} → ${groupName(now.group_id)}`);
  if (was.folder !== now.folder) out.push('folder');
  return [...out, ...checkAndLimitChanges(was.advanced, now.advanced)];
}

function checkAndLimitChanges(was: DraftAgent['advanced'], now: DraftAgent['advanced']): string[] {
  const out: string[] = [];
  if (was.check !== now.check) out.push('check');
  if (was.token_limit?.tokens !== now.token_limit?.tokens) out.push('token limit');
  return out;
}

function agentRows(before: TeamDraft, after: TeamDraft): ChangeRow[] {
  const groupName = (id: string | null) => after.groups.find((g) => g.id === id)?.label ?? before.groups.find((g) => g.id === id)?.label ?? 'none';
  const rows = after.agents.flatMap((now): ChangeRow[] => {
    const was = before.agents.find((a) => a.id === now.id);
    if (!was) return [{ change: 'added', target: { kind: 'agent', id: now.id }, what: now.name, details: [] }];
    const details = agentDetails(was, now, groupName);
    return details.length ? [{ change: 'changed', target: { kind: 'agent', id: now.id }, what: now.name, details, before: was, after: now }] : [];
  });
  const gone = before.agents.filter((a) => !after.agents.some((n) => n.id === a.id));
  return [...rows, ...gone.map((a): ChangeRow => ({ change: 'removed', target: { kind: 'agent', id: a.id }, what: a.name, details: [] }))];
}

function groupRows(before: TeamDraft, after: TeamDraft): ChangeRow[] {
  const rows = after.groups.flatMap((now): ChangeRow[] => {
    const was = before.groups.find((g) => g.id === now.id);
    if (!was) return [{ change: 'added', target: { kind: 'group', id: now.id }, what: `group ${now.label}`, details: [] }];
    return was.label !== now.label ? [{ change: 'changed', target: { kind: 'group', id: now.id }, what: `group ${now.label}`, details: [`name: ${was.label} → ${now.label}`] }] : [];
  });
  const gone = before.groups.filter((g) => !after.groups.some((n) => n.id === g.id));
  return [...rows, ...gone.map((g): ChangeRow => ({ change: 'removed', target: { kind: 'group', id: g.id }, what: `group ${g.label}`, details: [] }))];
}

function arrowRows(before: TeamDraft, after: TeamDraft): ChangeRow[] {
  const name = (id: string) => after.agents.find((a) => a.id === id)?.name ?? before.agents.find((a) => a.id === id)?.name ?? after.groups.find((g) => g.id === id)?.label ?? id;
  const words = (c: DraftConnection) => `${name(c.from)} → ${name(c.to)}`;
  const added = after.connections.filter((c) => !before.connections.some((b) => b.id === c.id || sameEnds(b, c)));
  const removed = before.connections.filter((b) => !after.connections.some((c) => c.id === b.id || sameEnds(b, c)));
  return [
    ...added.map((c): ChangeRow => ({ change: 'added', target: { kind: 'connection', id: c.id }, what: words(c), details: [] })),
    ...removed.map((c): ChangeRow => ({ change: 'removed', target: { kind: 'connection', id: c.id }, what: words(c), details: [] })),
  ];
}

function teamRows(before: TeamDraft, after: TeamDraft): ChangeRow[] {
  const rows: ChangeRow[] = [];
  if (before.name !== after.name) rows.push({ change: 'changed', target: null, what: 'project name', details: [`${before.name} → ${after.name}`] });
  const [was, now] = [before.input?.text ?? null, after.input?.text ?? null];
  if (was !== now) rows.push({ change: was === null ? 'added' : now === null ? 'removed' : 'changed', target: null, what: 'the Input', details: [] });
  return rows;
}

/** What approving `after` would change in `before`, by id. */
export function compareDesigns(before: TeamDraft, after: TeamDraft): Comparison {
  const rows = [...agentRows(before, after), ...groupRows(before, after), ...arrowRows(before, after), ...teamRows(before, after)];
  const marks: Marks = {};
  for (const r of rows) if (r.target) marks[r.target.id] = r.change;
  const removed = <T extends { id: string }>(list: T[]) => list.filter((x) => marks[x.id] === 'removed');
  const shown: TeamDraft = {
    ...after,
    agents: [...after.agents, ...removed<DraftAgent>(before.agents)],
    groups: [...after.groups, ...removed<DraftGroup>(before.groups)],
    connections: [...after.connections, ...removed<DraftConnection>(before.connections)],
  };
  return { marks, rows, shown };
}
