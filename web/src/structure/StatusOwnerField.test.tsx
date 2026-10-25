import { expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { TeamDraft } from '../generated/TeamDraft';
import { draftAgent, teamDraft } from '../test-fixtures';
import type { Edit } from './Panels';
import { StatusOwnerField } from './StatusOwnerField';

it('says who writes the status page and lets you choose this agent', async () => {
  const d = teamDraft({ agents: [draftAgent('builder', { name: 'Builder' }), draftAgent('reviewer', { name: 'Reviewer' })], connections: [{ id: 'c', from: 'builder', to: 'reviewer', kind: 'hands-work-to' }] });
  let next: TeamDraft | null = null;
  const edit: Edit = vi.fn((f) => { const r = f(d); next = 'draft' in r ? r.draft : r; return null; });
  render(<StatusOwnerField draft={d} a={d.agents[0]} edit={edit} locked={false} />);
  expect(screen.getByText('Now: Reviewer writes it (chosen automatically).')).toBeInTheDocument();
  await userEvent.click(screen.getByRole('checkbox', { name: /This agent writes the team's status page/ }));
  expect(next!.agents.map((a) => !!a.advanced.writes_status)).toEqual([true, false]);
});
