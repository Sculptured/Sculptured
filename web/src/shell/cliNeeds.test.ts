import { expect, it } from 'vitest';
import { draftAgent, teamDraft } from '../test-fixtures';
import { cliNeeds } from './CliReadiness';

it('an agent on a model connection (API key or OpenRouter) needs no CLI sign-in; one on a subscription does', () => {
  const onKey = { ...draftAgent('a'), kind: 'claude' as const, connection: 'conn-1' };
  const onSub = { ...draftAgent('b'), kind: 'claude' as const };
  const codex = { ...draftAgent('c'), kind: 'codex' as const };
  expect(cliNeeds(teamDraft({ agents: [onKey] }))).toEqual([]);
  expect(cliNeeds(teamDraft({ agents: [onKey, onSub] }))).toEqual(['claude']);
  expect(cliNeeds(teamDraft({ agents: [onKey, codex] }))).toEqual(['codex']);
});
