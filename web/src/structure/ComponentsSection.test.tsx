import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EditResult } from '../useTeamDraft';
import { teamDraft } from '../test-fixtures';
import { ComponentsSection } from './ComponentsSection';
import { ComponentChips } from './ComponentChips';
import { IntegrationFields } from './SkillFields';
import { ComponentBlockCard } from '../release/ComponentBlockCard';
import { api } from '../api';

vi.mock('../release/Community', () => ({ useCommunitySession: () => ({ session: { user: null, csrf: null } }), communityRequest: vi.fn() }));

const notes = { id: 'notes', name: 'Notes server', description: 'Team notes.', kind: 'mcp', program: 'notes-mcp', install: 'pip install notes-mcp' };

function Harness() {
  const [d, setD] = useState<TeamDraft>(teamDraft({ id: 't', name: 'T' }));
  const edit = (f: (d: TeamDraft) => EditResult) => { const r = f(d); setD('draft' in r ? r.draft : r); return null; };
  const a = d.agents[0];
  return <>
    <ComponentsSection draft={d} edit={edit} locked={false} />
    <IntegrationFields agent={a} edit={edit} locked={false} draft={d} />
    <div data-testid="card"><ComponentChips agent={a} draft={d} /></div>
    <pre data-testid="s">{JSON.stringify({ components: (d.components ?? []).map((c) => c.id), uses: a.advanced.integrations ?? [] })}</pre>
  </>;
}
const saved = () => JSON.parse(screen.getByTestId('s').textContent!);

describe('Components', () => {
  it('lists the built-in ones, adds a valid manifest, refuses a bad one, and an agent gets it as a chip', () => {
    render(<Harness />);
    expect(within(screen.getByRole('group', { name: 'Components' })).getByText('Kindex memory')).toBeTruthy();
    const box = screen.getByRole('textbox', { name: /paste its manifest/ });
    fireEvent.change(box, { target: { value: JSON.stringify({ ...notes, program: '/bin/sh' }) } });
    fireEvent.click(screen.getByRole('button', { name: 'Add component' }));
    expect(screen.getByRole('alert').textContent).toMatch(/plain program name/);
    fireEvent.change(box, { target: { value: JSON.stringify(notes) } });
    fireEvent.click(screen.getByRole('button', { name: 'Add component' }));
    expect(saved().components).toEqual(['notes']);
    expect(within(screen.getByRole('group', { name: 'Components' })).getByText('notes-mcp')).toBeTruthy();
    fireEvent.click(screen.getByRole('checkbox', { name: /Notes server/ }));
    expect(saved().uses).toEqual(['notes']);
    expect(screen.getByTestId('card').textContent).toContain('Notes server');
    fireEvent.click(screen.getByRole('button', { name: 'Remove component Notes server' }));
    expect(saved().components).toEqual([]);
  });

  it('shows a shared component with the command it runs and adds it to a team', async () => {
    vi.spyOn(api, 'listTeams').mockResolvedValue([{ id: 'b', name: 'Builders', agent_count: 1, updated_at_ms: 0 } as never]);
    vi.spyOn(api, 'getTeam').mockResolvedValue({ draft: teamDraft({ id: 'b', name: 'Builders' }) } as never);
    const save = vi.spyOn(api, 'saveTeam').mockResolvedValue({} as never);
    render(<ComponentBlockCard block={{ id: 'x', kind: 'component', title: 'Notes server', description: 'Team notes.', author: 'Ana', version: 1, license: 'MIT', tags: ['component'], content: { kind: 'component', component: notes as never } }} />);
    expect(screen.getByText('notes-mcp')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Add to a team' }));
    fireEvent.change(await screen.findByRole('combobox', { name: 'Add to which team' }), { target: { value: 'b' } });
    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(save.mock.calls[0][2].components?.[0].id).toBe('notes');
  });
});
