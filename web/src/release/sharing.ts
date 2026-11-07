import type { GroupBlock } from "../generated/GroupBlock";
type Block = GroupBlock;
type Draft = Block['draft'];

const validString = (v: unknown) => typeof v === 'string' && v.length <= 200000;
const AGENT_KINDS = ['claude', 'codex', 'opencode', 'practice'];
const CONNECTION_KINDS = ['hands-work-to', 'reports-to', 'watches'];

const hasShape = (b: Block) => !!b?.draft && Array.isArray(b.draft.groups) && b.draft.groups.length === 1 && Array.isArray(b.draft.agents) && b.draft.agents.length <= 64;

const groupOk = (b: Block, g: Draft['groups'][number] | undefined) =>
  validString(b.id) && validString(b.name) && !!g && validString(g.id) && validString(g.label) && [g.x, g.y, g.w, g.h].every(Number.isFinite) && g.w > 0 && g.h > 0;

const agentOk = (a: Draft['agents'][number], groupId: string) =>
  !!a && validString(a.id) && validString(a.name) && validString(a.job) && AGENT_KINDS.includes(a.kind) && a.group_id === groupId && [a.x, a.y].every(Number.isFinite);

const connectionOk = (c: Draft['connections'][number], ids: Set<string>) =>
  !!c && validString(c.id) && ids.has(c.from) && ids.has(c.to) && CONNECTION_KINDS.includes(c.kind);

function isValidExport(b: Block, ids: Set<string>): boolean {
  const d = b.draft, g = d.groups[0];
  return groupOk(b, g) && ids.size === d.agents.length && d.agents.every(a => agentOk(a, g.id))
    && Array.isArray(d.connections) && d.connections.length <= 500 && d.connections.every(c => connectionOk(c, ids));
}

/** Strips what stays on this computer: folders, models, permissions, the workflow input and where a copy came from. */
function stripLocal(b: Block) {
  b.revision = 0; b.boundary_notes = [];
  delete b.source; // where a Library copy came from stays on this computer
  b.draft.revision = 0;
  delete b.draft.folder;
  delete b.draft.models;
  for (const g of b.draft.groups) delete g.models;
  for (const a of b.draft.agents) {
    a.folder = null;
    a.advanced = { ...a.advanced, model: null, permission: 'restricted', resume: 'relaunch-fresh' };
  }
  if (b.draft.workflow) b.draft.workflow.input = '';
}

export function sanitizeExport(input: GroupBlock): GroupBlock {
  const b = structuredClone(input);
  if (!hasShape(b)) throw Error('Choose a group export from Sculptured Library');
  const d = b.draft;
  const ids = new Set(d.agents.map(a => a?.id));
  if (!isValidExport(b, ids)) throw Error('Choose a valid group export from Sculptured Library');
  if (d.workflow) checkWorkflow(d.workflow, ids, d.connections.length);
  stripLocal(b);
  return b;
}
export function downloadBlock(b: GroupBlock) {
  const blob = new Blob([JSON.stringify(sanitizeExport(b), null, 2)], {
      type: "application/json",
    }),
    url = URL.createObjectURL(blob),
    a = document.createElement("a");
  a.href = url;
  a.download = `${b.id}.group.json`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

type Workflow = NonNullable<Draft['workflow']>;
type Stage = Workflow['stages'][number];

const isAgentless = (st: Stage) => (st.kind !== undefined && st.kind !== 'agent') || (st.reviewers?.length ?? 0) > 0;
const inGroup = (l: unknown, ids: Set<string>) => l === undefined || (Array.isArray(l) && l.every(id => ids.has(id)));
const plainObject = (v: unknown) => !!v && typeof v === 'object' && !Array.isArray(v);

function stageOk(st: Stage, ids: Set<string>, stages: Set<string>) {
  const agentOk = isAgentless(st) ? !st.agent || ids.has(st.agent) : ids.has(st.agent);
  return !!st && validString(st.id) && validString(st.name) && agentOk && inGroup(st.reviewers, ids) && inGroup(st.workers, ids)
    && plainObject(st.routes) && Object.values(st.routes).every(to => to === null || stages.has(to));
}

/** As Community checks it: stages with unique ids and routes inside the workflow; check, person and merge
 * stages and review panels have no single agent; every agent a stage names is in the group. */
function checkWorkflow(w: Workflow, ids: Set<string>, connections: number) {
  if (!Array.isArray(w.stages) || w.stages.length > 100 || connections) throw Error('Invalid group workflow');
  const stages = new Set(w.stages.map(s => s?.id));
  if (stages.size !== w.stages.length || !w.stages.every(s => stageOk(s, ids, stages))) throw Error('Invalid group workflow references');
}
