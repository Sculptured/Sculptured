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
import * as registry from './mcpRegistry';

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
    expect(within(screen.getByRole('group', { name: 'Components' })).getByText(/Runs notes-mcp on the agent's computer/)).toBeTruthy();
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
    expect(screen.getByText(/Runs notes-mcp/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Add to a team' }));
    fireEvent.change(await screen.findByRole('combobox', { name: 'Add to which team' }), { target: { value: 'b' } });
    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(save.mock.calls[0][2].components?.[0].id).toBe('notes');
  });

  it('makes a remote MCP server from the form, keeping its secret as a name, and the picker groups it', () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'New component' }));
    const form = within(screen.getByRole('group', { name: 'New component' }));
    fireEvent.change(form.getByRole('textbox', { name: 'Name' }), { target: { value: 'OpenFOAM' } });
    fireEvent.change(form.getByRole('textbox', { name: 'Address' }), { target: { value: 'https://foam.example.com/mcp' } });
    fireEvent.change(form.getByRole('textbox', { name: 'Header 1 value' }), { target: { value: 'Bearer s3cret' } });
    fireEvent.change(form.getByRole('textbox', { name: 'How to set it up on a computer' }), { target: { value: 'Ask the HPC team for a token' } });
    fireEvent.click(form.getByRole('button', { name: 'Add' }));
    expect(screen.getByRole('alert').textContent).toMatch(/must name its secret/);
    fireEvent.change(form.getByRole('textbox', { name: 'Header 1 value' }), { target: { value: 'Bearer ${FOAM_TOKEN}' } });
    fireEvent.click(form.getByRole('button', { name: 'Add' }));
    expect(saved().components).toEqual(['openfoam']);
    const list = within(screen.getByRole('group', { name: 'Components' }));
    expect(list.getByText(/connects to https:\/\/foam.example.com\/mcp/).textContent).toContain('sculptured set-secret FOAM_TOKEN');
    const servers = within(screen.getByRole('group', { name: 'MCP servers' }));
    fireEvent.click(servers.getByRole('checkbox', { name: /OpenFOAM/ }));
    expect(saved().uses).toEqual(['openfoam']);
    expect(within(screen.getByRole('group', { name: 'Plugins and hooks' })).getByRole('checkbox', { name: /Signet/ })).toBeTruthy();
  });

  it('takes an MCP server from the official registry, filled into the form to check before it is added', async () => {
    const search = vi.spyOn(registry, 'searchRegistry').mockResolvedValue([{ name: 'io.github.iowarp/slurm-mcp', description: 'Slurm workload management', version: '2.2.0', packages: [{ registryType: 'pypi', identifier: 'clio-kit', version: '2.2.0', transport: { type: 'stdio' } }] }]);
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'From the MCP Registry' }));
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search the MCP Registry' }), { target: { value: 'slurm' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(search).toHaveBeenCalledWith('slurm');
    fireEvent.click(await screen.findByRole('button', { name: 'Use slurm-mcp' }));
    const form = screen.getByRole('group', { name: 'New component' });
    expect(within(form).getByRole('textbox', { name: 'Program' })).toHaveValue('uvx');
    expect(within(form).getByRole('textbox', { name: 'Arguments' })).toHaveValue('clio-kit==2.2.0');
    expect(saved().components).toEqual([]);
    fireEvent.click(within(form).getByRole('button', { name: 'Add' }));
    expect(saved().components).toEqual(['slurm-mcp']);
  });

  it('offers a Claude plugin to Claude agents only', () => {
    const plugin = { id: 'foam-plugin', name: 'Foam plugin', description: 'Case templates.', kind: 'claude-plugin', plugin: 'foam@hpc', claude_only: true, install: 'claude plugin install foam@hpc' };
    const d = teamDraft({ id: 't', name: 'T' });
    const codex = { ...d.agents[0], kind: 'codex' as const };
    render(<IntegrationFields agent={codex} edit={() => null} locked={false} draft={{ ...d, components: [plugin as never] }} />);
    const box = screen.getByRole('checkbox', { name: /Foam plugin/ }) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(box.closest('label')!.textContent).toContain('Claude Code agents only');
  });
});
