import { act,fireEvent,render,screen,waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach,expect,it,vi } from 'vitest';
import { api } from '../api';
import { GroupLibrary } from './GroupLibrary';
import { GroupPalette } from './GroupPalette';
import { captureGroup } from './blocks';
import { teamDraft,draftAgent } from '../test-fixtures';
import { resetApi } from '../structure/harness';
import { forgetViewports } from '../shell/viewport';
vi.mock('../api',async orig=>{const m=await orig<typeof import('../api')>();return {...m,api:{...m.api,groupBlocks:vi.fn(),saveGroupBlock:vi.fn(),createTeam:vi.fn(),saveTeam:vi.fn(),listRigs:vi.fn(),listTeams:vi.fn(),latestSeatEvents:vi.fn(),templates:vi.fn(),listFolders:vi.fn()}};});
beforeEach(()=>{resetApi();localStorage.clear();forgetViewports();vi.mocked(api.groupBlocks).mockResolvedValue([]);vi.mocked(api.saveGroupBlock).mockImplementation(async b=>({...b,revision:b.revision+1}));});
it('creates and edits a group on its own board and saves to Library without creating a project',async()=>{
 const user=userEvent.setup();render(<MemoryRouter><GroupLibrary/></MemoryRouter>);
 await user.click(await screen.findByRole('button',{name:'Create group'}));
 await user.clear(screen.getByLabelText('Group name'));await user.type(screen.getByLabelText('Group name'),'Review pair');
 await user.click(screen.getByRole('button',{name:'Add a Claude agent'}));await user.click(screen.getByRole('button',{name:'Add a Claude agent'}));
 const cards=screen.getAllByRole('button',{name:/Claude \d —/});
 fireEvent.focus(cards[0]);fireEvent.keyDown(cards[0],{key:'ArrowRight'});
 await user.click(screen.getByRole('button',{name:'Save group'}));
 await screen.findByText('Saved in Library');
 expect(api.createTeam).not.toHaveBeenCalled();expect(api.saveTeam).not.toHaveBeenCalled();
 const b=vi.mocked(api.saveGroupBlock).mock.calls[0][0];expect(b.name).toBe('Review pair');expect(b.draft.agents).toHaveLength(2);expect(b.draft.agents.every(a=>a.group_id===b.draft.groups[0].id)).toBe(true);
});
it('loads a saved group into the palette and supports keyboard insertion',async()=>{
 const b=captureGroup(teamDraft({groups:[{id:'g',label:'Pair',x:0,y:0,w:500,h:300}],agents:[draftAgent('builder',{group_id:'g'}),draftAgent('reviewer',{group_id:'g'})]}),'g');
 vi.mocked(api.groupBlocks).mockResolvedValue([b]);const add=vi.fn(),drag=vi.fn();render(<MemoryRouter><GroupPalette draft={teamDraft()} locked={false} onAdd={add} onDrag={drag}/></MemoryRouter>);
 await userEvent.click(screen.getByRole('button',{name:'Saved groups'}));const button=await screen.findByRole('button',{name:'Pair · 2 agents'});
 act(()=>button.focus());await userEvent.keyboard('{Enter}');expect(add).toHaveBeenCalledWith(b);
});
it('keeps a failed save in the editor instead of claiming it was saved',async()=>{
 vi.mocked(api.saveGroupBlock).mockRejectedValue(new Error('offline'));render(<MemoryRouter><GroupLibrary/></MemoryRouter>);
 await userEvent.click(await screen.findByRole('button',{name:'Create group'}));await userEvent.click(screen.getByRole('button',{name:'Save group'}));
 await waitFor(()=>expect(screen.getByRole('alert')).toBeInTheDocument());expect(screen.getByText('Unsaved changes')).toBeInTheDocument();
});
it('a group saved from Community links back to its block there', async () => {
  const b = { ...captureGroup(teamDraft({ groups: [{ id: 'g', label: 'Pair', x: 0, y: 0, w: 600, h: 300 }], agents: [draftAgent('builder', { group_id: 'g' })], connections: [] }), 'g'), id: 'community-b1', revision: 1, source: { community_block: 'b1', version: 1 } };
  vi.mocked(api.groupBlocks).mockResolvedValue([b]);
  render(<MemoryRouter><GroupLibrary /></MemoryRouter>);
  expect(await screen.findByRole('link', { name: 'From Community · v1' })).toHaveAttribute('href', '/community/blocks/b1');
});
