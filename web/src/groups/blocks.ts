import type { GroupBlock } from '../generated/GroupBlock';
import type { TeamDraft } from '../generated/TeamDraft';
import { groupOf, newId, slug } from '../draft/ids';
import { drawnGroups, snap, type Point } from '../draft/geometry';

export function emptyBlock(): GroupBlock {
  const id=newId('block',new Set());
  return {id,name:'New group',revision:0,boundary_notes:[],draft:{id,name:'New group',revision:0,models:{claude:null,codex:null},groups:[{id:'group',label:'New group',x:40,y:40,w:540,h:360}],agents:[],connections:[]}};
}

/** Copy only the selected group; keep boundary requirements explicit rather than stale IDs. */
export function captureGroup(d:TeamDraft, groupId:string):GroupBlock {
  const g=drawnGroups(d).find(g=>g.id===groupId);
  if (!g) throw new Error('Group not found');
  const block=emptyBlock(), members=d.agents.filter(a=>groupOf(a)===groupId), ids=new Set(members.map(a=>a.id));
  const stages=d.workflow?.stages.filter(s=>ids.has(s.agent)) ?? [], stageIds=new Set(stages.map(s=>s.id));
  const boundary_notes=d.connections.filter(c=>ids.has(c.from)!==ids.has(c.to)).map(c=>`${d.agents.find(a=>a.id===c.from)?.name} → ${d.agents.find(a=>a.id===c.to)?.name} (${c.kind})`);
  if(d.workflow && stageIds.has(d.workflow.entry)) boundary_notes.push(`Connect an upstream dispatcher or stage to ${stages.find(s=>s.id===d.workflow!.entry)?.name}.`);
  for(const s of stages) for(const worker of s.workers??[]) if(!ids.has(worker)) boundary_notes.push(`${s.name}: reconnect external worker ${d.agents.find(a=>a.id===worker)?.name??worker}.`);
  const keptStages=stages.map(s=>({...s,workers:s.workers?.filter(a=>ids.has(a)),routes:Object.fromEntries(Object.entries(s.routes).filter(([outcome,to])=>{
    if(to!==null && !stageIds.has(to)) {boundary_notes.push(`${s.name}: ${outcome} → ${d.workflow?.stages.find(s=>s.id===to)?.name ?? to}`);return false;} return true;
  }))}));
  for(const s of d.workflow?.stages ?? []) for(const [outcome,to] of Object.entries(s.routes)) if(!stageIds.has(s.id) && to && stageIds.has(to)) boundary_notes.push(`${s.name}: ${outcome} → ${stages.find(s=>s.id===to)?.name}`);
  return {...block,name:g.label,boundary_notes,draft:{...block.draft,name:g.label,models:d.models,groups:[{...g,x:40,y:40,models:{claude:g.models?.claude ?? d.models?.claude ?? null,codex:g.models?.codex ?? d.models?.codex ?? null}}],
    agents:members.map(a=>({...a,group_id:g.id,advanced:{...a.advanced},folder:null,x:a.x-g.x+40,y:a.y-g.y+40})),connections:d.connections.filter(c=>ids.has(c.from)&&ids.has(c.to)).map(c=>({...c})),
    ...(keptStages.length && d.workflow ? {workflow:{...d.workflow,input:'',input_position:undefined,dispatcher:'',collector:'',entry:keptStages[0].id,stages:keptStages}} : {})}};
}
export function blockCompatibility(d:TeamDraft,b:GroupBlock):string|null {
  if(b.draft.workflow && !d.workflow && d.connections.length) return 'This group has workflow stages. Add it to a workflow project.';
  if(d.workflow && b.draft.connections.length) return 'This group uses ordinary connections. Add it to a project with ordinary connections.';
  return null;
}
/** Each insertion gets fresh IDs, preserving internal arrows/loops without linking copies. */
/** `keepFolders`: a copy pasted on a board keeps its agents' folders (a Library block has none). */
export function insertBlock(d:TeamDraft,b:GroupBlock,at?:Point,keepFolders=false):{draft:TeamDraft;id?:string;reason?:string} {
  const reason=blockCompatibility(d,b); if(reason)return {draft:d,reason};
  const uniqueName=(base:string,used:Set<string>)=>{const m=/^(.*?)(?:\s+(\d+))?$/.exec(base.trim())!;let name=base,n=Number(m[2]??1)+1;while(used.has(name.trim().toLowerCase()))name=`${m[1]} ${n++}`;used.add(name.trim().toLowerCase());return name;};
  const agentNames=new Set(d.agents.map(a=>a.name.trim().toLowerCase())),groupNames=new Set(d.groups.map(g=>g.label.trim().toLowerCase())),stageNames=new Set(d.workflow?.stages.map(s=>s.name.trim().toLowerCase())??[]);
  const source=b.draft,g=source.groups[0]; if(!g)return {draft:d,reason:'This saved group has no group block.'};
  const used=new Set([...d.groups.map(g=>g.id),...d.agents.map(a=>a.id),...d.connections.map(c=>c.id),...(d.workflow?.stages.map(s=>s.id)??[])]);
  const fresh=(base:string)=>{const id=newId(slug(base,'item'),used);used.add(id);return id;};
  const groupId=fresh(b.name), agents=new Map(source.agents.map(a=>[a.id,fresh(a.id)])), stages=new Map((source.workflow?.stages??[]).map(s=>[s.id,fresh(s.id)]));
  // A person's or a check's stage has no agent (''); every other reference follows its agent's new id.
  const agentOf=(id:string)=>id ? agents.get(id) ?? id : id;
  const x=snap(at?.x ?? Math.max(0,...d.agents.map(a=>a.x+200),...d.groups.map(g=>g.x+g.w))+80),y=snap(at?.y ?? 40);
  return {id:groupId,draft:{...d,groups:[...d.groups,{...g,id:groupId,label:uniqueName(b.name,groupNames),x,y}],agents:[...d.agents,...source.agents.map(a=>({...a,id:agents.get(a.id)!,name:uniqueName(a.name,agentNames),group_id:groupId,folder:keepFolders?a.folder:null,x:x+a.x-g.x,y:y+a.y-g.y,advanced:{...a.advanced}}))],
    connections:[...d.connections,...source.connections.map(c=>({...c,id:fresh(c.id),from:agents.get(c.from)!,to:agents.get(c.to)!}))],
    ...(source.workflow ? {workflow:{...(d.workflow??{...source.workflow,input:'',input_position:undefined,dispatcher:agents.get(source.workflow.dispatcher)??'',collector:agents.get(source.workflow.collector)??'',entry:stages.get(source.workflow.entry)??''}),stages:[...(d.workflow?.stages??[]),...source.workflow.stages.map(s=>({...s,id:stages.get(s.id)!,name:uniqueName(s.name,stageNames),agent:agentOf(s.agent),workers:s.workers?.map(agentOf),reviewers:s.reviewers?.map(agentOf),escalate:s.escalate?{...s.escalate,agent:agentOf(s.escalate.agent)}:s.escalate,on_limit:s.on_limit?stages.get(s.on_limit)??s.on_limit:s.on_limit,routes:Object.fromEntries(Object.entries(s.routes).map(([outcome,to])=>[outcome,to===null?null:stages.get(to)!]))}))]}} : {}),
    ...(source.schedules?.length ? {schedules:[...(d.schedules??[]),...source.schedules.map(t=>({...t,id:fresh(t.id),agent:agentOf(t.agent)}))]} : {})}};
}
