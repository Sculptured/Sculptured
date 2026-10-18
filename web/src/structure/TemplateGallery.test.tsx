import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { draftAgent, templateInfos } from '../test-fixtures';
import { openRigWords } from '../test-vocab';
import { plainSummary, previewNameLines, TemplateGallery } from './TemplateGallery';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, templates: vi.fn(), createTeam: vi.fn(), saveTeam: vi.fn() } };
});

const extra = [
  { id: 'essay-committee', title: 'Independent reviewers', summary: 'Two independent perspectives, then an editor synthesizes the evidence. CS2680 committee.', default_name: 'Independent reviewers', preview: { ...templateInfos()[0].preview, agents: [draftAgent('a', { kind: 'claude', name: 'Evidence reviewer' })] } },
  { id: 'support-triage', title: 'Adaptive support team', summary: 'A coordinator creates billing and log specialists only when needed. CS2680 dynamic topology.', default_name: 'Adaptive support team', preview: { ...templateInfos()[0].preview, agents: [draftAgent('a', { kind: 'claude', name: 'Support coordinator' })] } },
];

beforeEach(() => {
  localStorage.clear();
  vi.mocked(api.templates).mockResolvedValue([...templateInfos(), ...extra]);
});

const show = () => render(<MemoryRouter><TemplateGallery inline onCreated={() => {}} /></MemoryRouter>);

describe('New project gallery', () => {
  it('leads with a few starting teams; the rest wait under More templates, which is remembered', async () => {
    show();
    const featured = await screen.findByRole('list', { name: 'Templates' });
    expect(within(featured).getAllByRole('button')).toHaveLength(4);
    for (const name of ['Solo agent', 'Builder → Reviewer', 'Planner → 2 Builders', 'Blank canvas']) expect(within(featured).getByRole('button', { name })).toBeInTheDocument();
    expect(within(featured).queryByRole('button', { name: 'Independent reviewers' })).toBeNull();
    const more = screen.getByText('More templates (2)').closest('details')!;
    expect(more).not.toHaveAttribute('open');
    await userEvent.click(screen.getByText('More templates (2)'));
    expect(more).toHaveAttribute('open');
    expect(localStorage.getItem('sculptured.gallery.more.v1')).toBe('true');
    expect(within(more).getByRole('button', { name: 'Independent reviewers' })).toHaveAccessibleDescription(/^Two independent perspectives, then an editor synthesizes the evidence\. 1 agent · Claude$/);
    // Describe it is the second way in; file import stays under Advanced.
    expect(screen.getByRole('button', { name: /Describe it/ })).toBeInTheDocument();
    expect(screen.getByText(/^Advanced/).closest('details')).toHaveAttribute('data-advanced');
    expect(screen.getByRole('region', { name: 'New project' })).toBeInTheDocument();
    expect(openRigWords(document.body)).toEqual([]);
  });

  it('never claims a kind the gallery will not create, and drops course references', () => {
    expect(plainSummary({ id: 'builder-reviewer', summary: 'Claude builds, then hands the work to Codex to review.' })).not.toMatch(/Codex|Claude/);
    expect(plainSummary({ id: 'design-review', summary: 'A designer and reviewer exchange bounded feedback. CS2680 pipeline with a retry edge.' })).toBe('A designer and reviewer exchange bounded feedback.');
    expect(plainSummary({ id: 'new-one', summary: 'Kept as the server says.' })).toBe('Kept as the server says.');
  });

  it('long agent names in a preview wrap onto two lines instead of being cut mid-word', () => {
    expect(previewNameLines('Claude')).toEqual({ lines: ['Claude'], small: false });
    expect(previewNameLines('Support coordinator')).toEqual({ lines: ['Support', 'coordinator'], small: true });
    expect(previewNameLines('Clarity reviewer')).toEqual({ lines: ['Clarity', 'reviewer'], small: true });
    expect(previewNameLines('Supercalifragilistic')).toEqual({ lines: ['Supercalifr…'], small: true });
  });
});
