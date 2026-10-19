import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it } from 'vitest';
import { draftAgent, teamDraft } from '../test-fixtures';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EditResult } from '../useTeamDraft';
import { BriefingField } from './BriefingField';
import { moreSummary } from './Panels';

let latest: TeamDraft;
function Harness() {
  const [d, set] = useState(teamDraft({ agents: [draftAgent('worker')] }));
  latest = d;
  const edit = (fn: (d: TeamDraft) => EditResult) => { const r = fn(d); set('draft' in r ? r.draft : r); return null; };
  return <BriefingField a={d.agents[0]} edit={edit} locked={false} />;
}

it('switches an agent between the full and the minimal briefing', async () => {
  const user = userEvent.setup();
  render(<Harness />);
  expect(screen.getByLabelText('Briefing')).toHaveValue('full');
  await user.selectOptions(screen.getByLabelText('Briefing'), 'minimal');
  expect(latest.agents[0].advanced.briefing).toBe('minimal');
  expect(moreSummary(latest.agents[0], undefined)).toBe('minimal briefing');
  await user.selectOptions(screen.getByLabelText('Briefing'), 'full');
  expect(latest.agents[0].advanced.briefing).toBeUndefined();
});
