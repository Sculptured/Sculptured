import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import type { Publication } from '../generated/community/Publication';
import { ExplorePage } from './ExplorePage';
import { fakeFetch, publication, renderCommunity } from './testing';

afterEach(() => vi.restoreAllMocks());

const kindex = publication({
  kind: 'component', id: 'component-kindex', title: 'Kindex memory', tags: ['component', 'kindex'],
  content: { kind: 'component', component: { id: 'kindex', name: 'Kindex memory', description: 'A knowledge graph that outlasts the team.', kind: 'mcp', program: 'kin-mcp', install: "pip install 'kindex[mcp]'" } } as unknown as Publication['content'],
});

it('a component says what it gives and runs, and can be added to a team', async () => {
  const { calls } = fakeFetch({
    'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [kindex], next: null }],
    'GET /api/teams': () => [200, [{ id: 't1', name: 'Docs team', agent_count: 2, updated_at_ms: 1 }]],
  });
  renderCommunity(<ExplorePage />, { path: '/community', state: { signedIn: true } });
  expect(await screen.findByText('A knowledge graph that outlasts the team.')).toBeVisible();
  expect(screen.getByText(/kin-mcp/)).toBeVisible();
  expect(screen.queryByText(/later version/)).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Add to a team' }));
  expect(await screen.findByRole('combobox', { name: 'Add to which team' })).toBeVisible();
  expect(calls).toContain('GET /api/teams');
});
