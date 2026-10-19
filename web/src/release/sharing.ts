import type { GroupBlock } from "../generated/GroupBlock";
export function sanitizeExport(input: GroupBlock): GroupBlock {
  const b = structuredClone(input);
  if (
    !b?.draft ||
    !Array.isArray(b.draft.groups) ||
    b.draft.groups.length !== 1 ||
    !Array.isArray(b.draft.agents) ||
    b.draft.agents.length > 64
  )
    throw Error("Choose a group export from Sculptured Library");
  const d=b.draft,g=d.groups[0];
  const validString=(v:unknown)=>typeof v==='string'&&v.length<=200000;
  const ids=new Set(d.agents.map(a=>a?.id));
  if (!validString(b.id)||!validString(b.name)||!g||!validString(g.id)||!validString(g.label)||![g.x,g.y,g.w,g.h].every(Number.isFinite)||g.w<=0||g.h<=0
      ||ids.size!==d.agents.length||d.agents.some(a=>!a||!validString(a.id)||!validString(a.name)||!validString(a.job)||!['claude','codex','practice'].includes(a.kind)||a.group_id!==g.id||![a.x,a.y].every(Number.isFinite))
      ||!Array.isArray(d.connections)||d.connections.length>500||d.connections.some(c=>!c||!validString(c.id)||!ids.has(c.from)||!ids.has(c.to)||!['hands-work-to','reports-to','watches'].includes(c.kind)))
    throw Error('Choose a valid group export from Sculptured Library');
  if(d.workflow) {
    const w=d.workflow;
    if(!Array.isArray(w.stages)||w.stages.length>100||d.connections.length)throw Error('Invalid group workflow');
    const stages=new Set(w.stages.map(s=>s?.id));
    if(stages.size!==w.stages.length||w.stages.some(s=>!s||!validString(s.id)||!validString(s.name)||!ids.has(s.agent)||!s.routes||typeof s.routes!=='object'||Array.isArray(s.routes)||Object.values(s.routes).some(to=>to!==null&&!stages.has(to))||s.workers!==undefined&&(!Array.isArray(s.workers)||s.workers.some(id=>!ids.has(id)))))throw Error('Invalid group workflow references');
  }
  b.revision = 0; b.boundary_notes = [];
  delete b.source; // where a Library copy came from stays on this computer
  b.draft.revision = 0;
  delete b.draft.folder;
  delete b.draft.models;
  for (const g of b.draft.groups) delete g.models;
  for (const a of b.draft.agents) {
    a.folder = null;
    a.advanced = {
      ...a.advanced,
      model: null,
      permission: "restricted",
      resume: "relaunch-fresh",
    };
  }
  if (b.draft.workflow) b.draft.workflow.input = "";
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
