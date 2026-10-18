import { beforeAll, describe, expect, it } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EditResult } from '../useTeamDraft';
import { teamDraft } from '../test-fixtures';
import { IntegrationFields, SkillFields } from './SkillFields';
import { parseSkillMd } from './skills';

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
});

function Harness() {
  const [d, setD] = useState<TeamDraft>(teamDraft({ id: 't', name: 'T' }));
  const edit = (f: (d: TeamDraft) => EditResult) => { const r = f(d); setD('draft' in r ? r.draft : r); return null; };
  const a = d.agents[0];
  return <><SkillFields agent={a} edit={edit} locked={false} /><IntegrationFields agent={a} edit={edit} locked={false} /><pre data-testid="s">{JSON.stringify({ skills: a.advanced.skills ?? [], integrations: a.advanced.integrations ?? [] })}</pre></>;
}
const saved = () => JSON.parse(screen.getByTestId('s').textContent!);

describe('An agent’s skills', () => {
  it('are written in a full-size editor that checks the name, and can be removed', () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'Add skill' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Money Rules' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'When to use it' }), { target: { value: 'Use when handling money.' } });
    fireEvent.change(screen.getByRole('textbox', { name: /Instructions/ }), { target: { value: 'Decimal only.' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save skill' }));
    expect(screen.getByRole('alert').textContent).toMatch(/lowercase letters/);
    expect(saved().skills).toEqual([]);
    fireEvent.change(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'money-rules' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save skill' }));
    expect(saved().skills).toEqual([{ name: 'money-rules', description: 'Use when handling money.', body: 'Decimal only.' }]);
    fireEvent.click(screen.getByRole('button', { name: 'Remove skill money-rules' }));
    expect(saved().skills).toEqual([]);
  });

  it('load from a SKILL.md, whose frontmatter names them', async () => {
    expect(parseSkillMd('---\nname: money-rules\ndescription: "Use for money."\n---\n\nDecimal only.\n')).toEqual({ name: 'money-rules', description: 'Use for money.', body: 'Decimal only.\n' });
    expect(parseSkillMd('just text')).toMatch(/starts with a ---/);
    render(<Harness />);
    const file = new File(['---\nname: tax-rules\ndescription: Use for tax.\n---\nRates are per region.\n'], 'SKILL.md', { type: 'text/markdown' });
    fireEvent.change(screen.getByLabelText('SKILL.md file'), { target: { files: [file] } });
    await waitFor(() => expect((screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('tax-rules'));
    fireEvent.click(screen.getByRole('button', { name: 'Save skill' }));
    expect(saved().skills[0].name).toBe('tax-rules');
  });

  it('Kindex is turned on and off per agent', () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole('checkbox', { name: /^Kindex memory/ }));
    expect(saved().integrations).toEqual(['kindex']);
    fireEvent.click(screen.getByRole('checkbox', { name: /^Kindex memory/ }));
    expect(saved().integrations).toEqual([]);
  });
});
