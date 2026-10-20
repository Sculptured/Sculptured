import { render,screen,within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter,Routes,Route,useLocation } from 'react-router-dom';
import { beforeEach,it,expect,vi } from 'vitest';
import { api } from '../api';
import { teamDraft,draftAgent,teamRecord } from '../test-fixtures';
import { captureGroup } from './blocks';
import { AddToProject } from './AddToProject';
import { useTeamList } from '../useTeamList';
import { BlockDetails,BlockDescription } from './BlockDetails';
import { removeElement } from '../draft/edits';
import { start,push,undo } from '../draft/history';
vi.mock('../api',async(orig)=>{const m=await orig<typeof import('../api')>();return {...m,api:{...m.api,listTeams:vi.fn(),projectVisibility:vi.fn(),createTeam:vi.fn(),getTeam:vi.fn(),saveTeam:vi.fn(),listFolders:vi.fn()}};});
const block=()=>captureGroup(teamDraft({groups:[{id:'g',label:'Review pair',x:0,y:0,w:800,h:320}],agents:[draftAgent('builder',{group_id:'g',job:'Build **something**.'}),draftAgent('reviewer',{group_id:'g',job:'Check the result.'})]}),'g');
beforeEach(()=>{vi.clearAllMocks();HTMLDialogElement.prototype.showModal=function(){this.open=true;};vi.mocked(api.projectVisibility).mockResolvedValue([]);vi.mocked(api.listTeams).mockResolvedValue([{id:'existing',name:'Existing',agent_count:0,updated_at_ms:0,started:null}]);});
function Destination(){return <p>{useLocation().pathname}</p>}
function ProjectList(){const {summaries}=useTeamList();return <nav aria-label="Test projects">{summaries?.map(t=><span key={t.id}>{t.name}</span>)}</nav>}
it('adds a saved block to an existing project and opens its board',async()=>{
 const record=teamRecord({draft:teamDraft({id:'existing',agents:[],connections:[],folder:'/project'})});vi.mocked(api.getTeam).mockResolvedValue(record);vi.mocked(api.saveTeam).mockResolvedValue(record);
 render(<MemoryRouter><Routes><Route path="/" element={<AddToProject block={block()} onClose={()=>{}}/>}/><Route path="*" element={<Destination/>}/></Routes></MemoryRouter>);
 await screen.findByRole('option',{name:'Existing'});await userEvent.selectOptions(screen.getByLabelText('Destination project'),'existing');await userEvent.click(screen.getByRole('button',{name:'Add & open project'}));
 await screen.findByText('/team/existing/structure');const saved=vi.mocked(api.saveTeam).mock.calls[0][2];expect(saved.agents).toHaveLength(2);expect(saved.connections).toHaveLength(1);expect(saved.folder).toBe('/project');expect(saved.agents.every(a=>a.folder===null)).toBe(true);expect(api.createTeam).not.toHaveBeenCalled();
});
it('does not add agents to a running project',async()=>{
 vi.mocked(api.getTeam).mockResolvedValue(teamRecord({live:true}));render(<MemoryRouter><AddToProject block={block()} onClose={()=>{}}/></MemoryRouter>);
 await screen.findByRole('option',{name:'Existing'});await userEvent.selectOptions(screen.getByLabelText('Destination project'),'existing');await userEvent.click(screen.getByRole('button',{name:'Add & open project'}));await screen.findByText('Stop this project before adding agents, or choose a new project.');expect(api.saveTeam).not.toHaveBeenCalled();
});
it('creates a blank project, inserts the group and navigates without starting it',async()=>{
 const record=teamRecord({draft:teamDraft({id:'new',agents:[],connections:[]})});vi.mocked(api.createTeam).mockResolvedValue(record);vi.mocked(api.getTeam).mockResolvedValue(record);vi.mocked(api.saveTeam).mockImplementation(async()=>{vi.mocked(api.listTeams).mockResolvedValue([{id:'new',name:'Newly imported project',agent_count:2,updated_at_ms:1,started:null}]);return record;});
 render(<MemoryRouter><ProjectList/><Routes><Route path="/" element={<AddToProject block={block()} onClose={()=>{}}/>}/><Route path="*" element={<Destination/>}/></Routes></MemoryRouter>);
 await userEvent.click(screen.getByRole('button',{name:'Add & open project'}));await screen.findByText('/team/new/structure');expect(api.createTeam).toHaveBeenCalledWith('blank',false,'Review pair',expect.any(String));
 expect(await within(screen.getByRole('navigation',{name:'Test projects'})).findByText('Newly imported project')).toBeVisible();
});
it('inspects individual agents and connections on the full board with linked source details',async()=>{
 render(<MemoryRouter><BlockDetails block={block()}/><BlockDescription text={'A review pair.\n\nArchitecture reference: Juncheng Yang, CS2680 Agent Design II, slide 15: https://cs2680.com/slides/5_agent_design_2.pdf#page=15. Original Sculptured instructions, not course-provided code or an endorsement. Reports-to/watch arrows describe intent; agents use managed messages to coordinate. Imported blocks require a folder and review; no agents start on import.'}/></MemoryRouter>);
 const list=screen.getByRole('navigation',{name:'Block components'});await userEvent.click(within(list).getByRole('button',{name:'Builder'}));expect(screen.getByRole('region',{name:'Selected component'})).toHaveTextContent('Build something.');await userEvent.click(within(list).getByRole('button',{name:'Builder → Reviewer'}));expect(screen.getByText('Hands completed work to the recipient.')).toBeVisible();
 await userEvent.click(screen.getByText('Background, source & usage notes'));expect(screen.getByRole('link',{name:/CS2680/})).toHaveAttribute('href','https://cs2680.com/slides/5_agent_design_2.pdf#page=15');
});
it('deleting a whole group cleans workflow references and undo restores the complete group',()=>{
 const b=block();const d=b.draft;d.agents.push(draftAgent('outside',{group_id:'other'}));d.workflow={input:'keep',dispatcher:'builder',collector:'outside',entry:'build',max_tasks:24,max_steps:12,stages:[{id:'build',name:'Build',agent:'builder',routes:{done:'review'}},{id:'review',name:'Review',agent:'outside',workers:['reviewer'],routes:{retry:'build',done:null}}]};
 const result=removeElement(d,{kind:'group',id:'g'}).draft;expect(result.agents.map(a=>a.id)).toEqual(['outside']);expect(result.connections).toEqual([]);expect(result.workflow?.stages[0]).toMatchObject({id:'review',workers:[],routes:{done:null}});expect(result.workflow?.entry).toBe('');expect(result.workflow?.dispatcher).toBe('');expect(result.workflow?.collector).toBe('outside');expect(undo(push(start(d),result)).present).toEqual(d);
});
