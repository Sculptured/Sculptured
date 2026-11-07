import type { GroupBlock } from '../generated/GroupBlock';
import type { TeamDraft } from '../generated/TeamDraft';
import { groupOf, newId, slug } from '../draft/ids';
import { drawnGroups, snap, type Point } from '../draft/geometry';

export function emptyBlock(): GroupBlock {
  const id=newId('block',new Set());
  return {id,name:'New group',revision:0,boundary_notes:[],draft:{id,name:'New group',revision:0,models:{claude:null,codex:null},groups:[{id:'group',label:'New group',x:40,y:40,w:540,h:360}],agents:[],connections:[]}};
}

type Draft = TeamDraft;
type Workflow = NonNullable<Draft['workflow']>;
type Stage = Workflow['stages'][number];

const nameOfAgent = (d: Draft, id: string) => d.agents.find(a => a.id === id)?.name;
const stageName = (stages: Stage[], id: string) => stages.find(s => s.id === id)?.name;

/** Connections that cross the group's edge become notes instead of stale ids. */
function boundaryConnections(d: Draft, ids: Set<string>) {
  return d.connections.filter(c => ids.has(c.from) !== ids.has(c.to)).map(c => `${nameOfAgent(d, c.from)} → ${nameOfAgent(d, c.to)} (${c.kind})`);
}

function boundaryEntry(d: Draft, stages: Stage[], stageIds: Set<string>) {
  return d.workflow && stageIds.has(d.workflow.entry) ? [`Connect an upstream dispatcher or stage to ${stageName(stages, d.workflow.entry)}.`] : [];
}

function externalWorkers(d: Draft, stages: Stage[], ids: Set<string>) {
  return stages.flatMap(s => (s.workers ?? []).filter(w => !ids.has(w)).map(w => `${s.name}: reconnect external worker ${nameOfAgent(d, w) ?? w}.`));
}

/** The stages a group keeps, without routes that leave it (those become notes). */
function keepStages(d: Draft, stages: Stage[], ids: Set<string>, stageIds: Set<string>, notes: string[]): Stage[] {
  return stages.map(s => ({ ...s, workers: s.workers?.filter(a => ids.has(a)), routes: Object.fromEntries(Object.entries(s.routes).filter(([outcome, to]) => {
    if (to !== null && !stageIds.has(to)) { notes.push(`${s.name}: ${outcome} → ${stageName(d.workflow?.stages ?? [], to) ?? to}`); return false; }
    return true;
  })) }));
}

function incomingRoutes(d: Draft, stages: Stage[], stageIds: Set<string>) {
  return (d.workflow?.stages ?? []).filter(s => !stageIds.has(s.id)).flatMap(s => Object.entries(s.routes)
    .filter(([, to]) => to && stageIds.has(to)).map(([outcome, to]) => `${s.name}: ${outcome} → ${stageName(stages, to!)}`));
}

function blockGroup(d: Draft, g: ReturnType<typeof drawnGroups>[number]) {
  const models = { claude: g.models?.claude ?? d.models?.claude ?? null, codex: g.models?.codex ?? d.models?.codex ?? null };
  return { ...g, x: 40, y: 40, models };
}

/** Copy only the selected group; keep boundary requirements explicit rather than stale IDs. */
export function captureGroup(d: Draft, groupId: string): GroupBlock {
  const g = drawnGroups(d).find(g => g.id === groupId);
  if (!g) throw new Error('Group not found');
  const block = emptyBlock(), members = d.agents.filter(a => groupOf(a) === groupId), ids = new Set(members.map(a => a.id));
  const stages = d.workflow?.stages.filter(s => ids.has(s.agent)) ?? [], stageIds = new Set(stages.map(s => s.id));
  const boundary_notes = [...boundaryConnections(d, ids), ...boundaryEntry(d, stages, stageIds), ...externalWorkers(d, stages, ids)];
  const keptStages = keepStages(d, stages, ids, stageIds, boundary_notes);
  boundary_notes.push(...incomingRoutes(d, stages, stageIds));
  const workflow = keptStages.length && d.workflow ? { workflow: { ...d.workflow, input: '', input_position: undefined, dispatcher: '', collector: '', entry: keptStages[0].id, stages: keptStages } } : {};
  return { ...block, name: g.label, boundary_notes, draft: { ...block.draft, name: g.label, models: d.models, groups: [blockGroup(d, g)],
    agents: members.map(a => ({ ...a, group_id: g.id, advanced: { ...a.advanced }, folder: null, x: a.x - g.x + 40, y: a.y - g.y + 40 })),
    connections: d.connections.filter(c => ids.has(c.from) && ids.has(c.to)).map(c => ({ ...c })), ...workflow } };
}

export function blockCompatibility(d: Draft, b: GroupBlock): string | null {
  if (b.draft.workflow && !d.workflow && d.connections.length) return 'This group has workflow stages. Add it to a workflow project.';
  if (d.workflow && b.draft.connections.length) return 'This group uses ordinary connections. Add it to a project with ordinary connections.';
  return null;
}

/** A name unused among `used`, counting up from the base's own number ("Writer", "Writer 2", ...). */
function uniqueName(base: string, used: Set<string>) {
  const m = /^(.*?)(?:\s+(\d+))?$/.exec(base.trim())!;
  let name = base, n = Number(m[2] ?? 1) + 1;
  while (used.has(name.trim().toLowerCase())) name = `${m[1]} ${n++}`;
  used.add(name.trim().toLowerCase());
  return name;
}

const lower = (names: string[]) => new Set(names.map(n => n.trim().toLowerCase()));

type Remap = { fresh: (base: string) => string; agents: Map<string, string>; stages: Map<string, string>; agentOf: (id: string) => string };

/** Fresh ids for everything a block brings, so copies never link to each other. */
function remapFor(d: Draft, b: GroupBlock): Remap {
  const source = b.draft;
  const used = new Set([...d.groups.map(g => g.id), ...d.agents.map(a => a.id), ...d.connections.map(c => c.id), ...(d.workflow?.stages.map(s => s.id) ?? [])]);
  const fresh = (base: string) => { const id = newId(slug(base, 'item'), used); used.add(id); return id; };
  const agents = new Map(source.agents.map(a => [a.id, fresh(a.id)]));
  const stages = new Map((source.workflow?.stages ?? []).map(s => [s.id, fresh(s.id)]));
  // A person's or a check's stage has no agent (''); every other reference follows its agent's new id.
  return { fresh, agents, stages, agentOf: (id: string) => (id ? agents.get(id) ?? id : id) };
}

function insertedStage(s: Stage, r: Remap, names: Set<string>): Stage {
  return { ...s, id: r.stages.get(s.id)!, name: uniqueName(s.name, names), agent: r.agentOf(s.agent), workers: s.workers?.map(r.agentOf), reviewers: s.reviewers?.map(r.agentOf),
    escalate: s.escalate ? { ...s.escalate, agent: r.agentOf(s.escalate.agent) } : s.escalate,
    on_limit: s.on_limit ? r.stages.get(s.on_limit) ?? s.on_limit : s.on_limit,
    routes: Object.fromEntries(Object.entries(s.routes).map(([outcome, to]) => [outcome, to === null ? null : r.stages.get(to)!])) };
}

function insertedWorkflow(d: Draft, source: Draft, r: Remap) {
  const sw = source.workflow;
  if (!sw) return {};
  const names = lower(d.workflow?.stages.map(s => s.name) ?? []);
  const base = d.workflow ?? { ...sw, input: '', input_position: undefined, dispatcher: r.agents.get(sw.dispatcher) ?? '', collector: r.agents.get(sw.collector) ?? '', entry: r.stages.get(sw.entry) ?? '' };
  return { workflow: { ...base, stages: [...(d.workflow?.stages ?? []), ...sw.stages.map(s => insertedStage(s, r, names))] } };
}

function insertedSchedules(d: Draft, source: Draft, r: Remap) {
  return source.schedules?.length ? { schedules: [...(d.schedules ?? []), ...source.schedules.map(t => ({ ...t, id: r.fresh(t.id), agent: r.agentOf(t.agent) }))] } : {};
}

/** Each insertion gets fresh IDs, preserving internal arrows/loops without linking copies. */
/** `keepFolders`: a copy pasted on a board keeps its agents' folders (a Library block has none). */
export function insertBlock(d: Draft, b: GroupBlock, at?: Point, keepFolders = false): { draft: Draft; id?: string; reason?: string } {
  const reason = blockCompatibility(d, b); if (reason) return { draft: d, reason };
  const source = b.draft, g = source.groups[0]; if (!g) return { draft: d, reason: 'This saved group has no group block.' };
  const r = remapFor(d, b), groupNames = lower(d.groups.map(g => g.label)), agentNames = lower(d.agents.map(a => a.name));
  const groupId = r.fresh(b.name);
  const x = snap(at?.x ?? Math.max(0, ...d.agents.map(a => a.x + 200), ...d.groups.map(g => g.x + g.w)) + 80), y = snap(at?.y ?? 40);
  const agents = source.agents.map(a => ({ ...a, id: r.agents.get(a.id)!, name: uniqueName(a.name, agentNames), group_id: groupId, folder: keepFolders ? a.folder : null, x: x + a.x - g.x, y: y + a.y - g.y, advanced: { ...a.advanced } }));
  const connections = source.connections.map(c => ({ ...c, id: r.fresh(c.id), from: r.agents.get(c.from)!, to: r.agents.get(c.to)! }));
  return { id: groupId, draft: { ...d, groups: [...d.groups, { ...g, id: groupId, label: uniqueName(b.name, groupNames), x, y }], agents: [...d.agents, ...agents],
    connections: [...d.connections, ...connections], ...insertedWorkflow(d, source, r), ...insertedSchedules(d, source, r) } };
}
