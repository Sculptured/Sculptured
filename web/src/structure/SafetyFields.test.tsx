import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import type { TeamDraft } from '../generated/TeamDraft';
import type { EditResult } from '../useTeamDraft';
import { teamDraft } from '../test-fixtures';
import { SafetyFields } from './SafetyFields';
import { ContractFields, parseShared } from './ContractFields';
import { IntegrationFields } from './SkillFields';

function Harness() {
  // Signet is a Claude Code tool hook, offered to Claude agents only.
  const [d, setD] = useState<TeamDraft>(() => { const t = teamDraft({ id: 't', name: 'T' }); return { ...t, agents: t.agents.map((a, i) => (i === 0 ? { ...a, kind: 'claude' as const } : a)) }; });
  const edit = (f: (d: TeamDraft) => EditResult) => { const r = f(d); setD('draft' in r ? r.draft : r); return null; };
  const a = d.agents[0];
  return <>
    <SafetyFields draft={d} edit={edit} locked={false} />
    <IntegrationFields agent={a} edit={edit} locked={false} />
    <ContractFields draft={d} agent={a} edit={edit} locked={false} />
    <pre data-testid="s">{JSON.stringify({ safety: d.safety ?? [], integrations: a.advanced.integrations ?? [], shared: a.advanced.data_access?.shared ?? [] })}</pre>
  </>;
}
const saved = () => JSON.parse(screen.getByTestId('s').textContent!);

describe('Safety and Signet', () => {
  it('adds a ready-made rule and your own, and removes one', () => {
    render(<Harness />);
    fireEvent.change(screen.getByRole('combobox', { name: 'Add a ready-made safety rule' }), { target: { value: 'no-force-push' } });
    expect(saved().safety.map((r: { name: string }) => r.name)).toEqual(['no-force-push']);
    fireEvent.click(screen.getByRole('button', { name: 'Add your own' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'no-drop' } });
    fireEvent.change(screen.getByRole('textbox', { name: /Conditions/ }), { target: { value: "contains(parameters, 'DROP TABLE')\n" } });
    fireEvent.click(screen.getByRole('button', { name: 'Add rule' }));
    expect(saved().safety[1]).toEqual({ name: 'no-drop', tool_pattern: '.*', conditions: ["contains(parameters, 'DROP TABLE')"] });
    fireEvent.click(screen.getByRole('button', { name: 'Remove safety rule no-force-push' }));
    expect(saved().safety.map((r: { name: string }) => r.name)).toEqual(['no-drop']);
  });

  it('turns Kindex on with Signet, and Signet off without Kindex', () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole('checkbox', { name: /Signet-Eval/ }));
    expect(saved().integrations).toEqual(['kindex', 'signet']);
    fireEvent.click(screen.getByRole('checkbox', { name: /^Kindex memory/ }));
    expect(saved().integrations).toEqual([]);
  });

  it('reads shared values as name = value @ file', () => {
    expect(parseShared('api port = 8080 @ server/config.py')).toEqual({ name: 'api port', value: '8080', file: 'server/config.py' });
    expect(parseShared('email = a@b.com @ users.py')?.value).toBe('a@b.com');
    expect(parseShared('no file = 1')).toBeNull();
    render(<Harness />);
    const box = screen.getByRole('textbox', { name: /Values it shares/ });
    fireEvent.change(box, { target: { value: 'port = 8080 @ config.py\nbroken line' } });
    fireEvent.blur(box);
    expect(saved().shared).toEqual([{ name: 'port', value: '8080', file: 'config.py' }]);
  });
});
