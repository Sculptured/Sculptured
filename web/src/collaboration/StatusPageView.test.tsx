import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { api, type TeamStatus } from '../api';
import { StatusPageView } from './StatusPageView';

vi.mock('../api', async (orig) => {
  const m = await orig<typeof import('../api')>();
  return { ...m, api: { ...m.api, teamStatus: vi.fn(), teamStatusVersion: vi.fn() } };
});

const owner = { id: 'reviewer', name: 'Reviewer', chosen: false };
const at = 1_800_000_000_000;
const latest = { seq: 9, at_ms: at, author: 'team.reviewer@t', author_name: 'Reviewer', markdown: '# Done\n\nAll **merged**.', done: true };

describe('Status page', () => {
  it('says who will write it before it exists', async () => {
    vi.mocked(api.teamStatus).mockResolvedValue({ owner, page: null, versions: [] });
    render(<StatusPageView rig="t" onClose={vi.fn()} />);
    expect(await screen.findByText('No status page yet.')).toBeInTheDocument();
    expect(screen.getByText(/Reviewer writes it while the team works.*chosen automatically/)).toBeInTheDocument();
  });

  it('shows the page as a document, who wrote it, and an earlier version on request', async () => {
    const s: TeamStatus = { owner, page: latest, versions: [{ ...latest, markdown: undefined } as never, { seq: 4, at_ms: at - 60_000, author: latest.author, author_name: 'Reviewer', done: false }] };
    vi.mocked(api.teamStatus).mockResolvedValue(s);
    vi.mocked(api.teamStatusVersion).mockResolvedValue({ ...latest, seq: 4, markdown: '# Building\n\nHalf done.', done: false });
    render(<StatusPageView rig="t" onClose={vi.fn()} />);
    const page = await screen.findByRole('article', { name: 'Status page' });
    expect(within(page).getByRole('heading', { name: 'Done' })).toBeInTheDocument();
    expect(within(page).getByText('merged').tagName).toBe('STRONG');
    expect(within(page).getByText('Final version')).toBeInTheDocument();
    expect(within(page).getByText('agent-reported')).toBeInTheDocument();
    await userEvent.selectOptions(screen.getByLabelText('Version'), '4');
    expect(api.teamStatusVersion).toHaveBeenCalledWith('t', 4);
    expect(await screen.findByRole('heading', { name: 'Building' })).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('An earlier version');
    await userEvent.click(screen.getByRole('button', { name: 'Show the latest' }));
    expect(await screen.findByRole('heading', { name: 'Done' })).toBeInTheDocument();
  });
});
