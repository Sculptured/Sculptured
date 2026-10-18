import { render, screen } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { vi } from 'vitest';
import { AppRoutes } from '../App';
import { api, ApiError } from '../api';
import type { FolderListing } from '../generated/FolderListing';
import type { TeamDraft } from '../generated/TeamDraft';
import type { TeamRecord } from '../generated/TeamRecord';
import { teamRecord, templateInfos } from '../test-fixtures';
import { openFake } from '../test-fixtures';
import { forgetBoardSelections } from './StructureBoard';

/** The api functions the Structure tests replace. Use with vi.mock('../api', mockApi). */
export const mockApi = async (orig: () => Promise<unknown>) => {
  const m = (await orig()) as typeof import('../api');
  return {
    ...m,
    api: {
      ...m.api,
      getTeam: vi.fn(),
      saveTeam: vi.fn(),
      listTeams: vi.fn(),
      templates: vi.fn(),
      createTeam: vi.fn(),
      startTeam: vi.fn(),
      stopTeam: vi.fn(),
      importTeam: vi.fn(),
      listFolders: vi.fn(),
      createFolder: vi.fn(),
      listRigs: vi.fn(),
      latestSeatEvents: vi.fn(),
      stopRig: vi.fn(),
    },
  };
};

export const HOME: FolderListing = {
  path: '/home/me',
  parent: null,
  home: '/home/me',
  entries: [
    { name: 'notes', path: '/home/me/notes' },
    { name: 'projects', path: '/home/me/projects' },
  ],
  truncated: false,
};
export const PROJECTS: FolderListing = {
  path: '/home/me/projects',
  parent: '/home/me',
  home: '/home/me',
  entries: [{ name: 'web-shop', path: '/home/me/projects/web-shop' }],
  truncated: false,
};

/** A fake server holding one team record: saves bump the revision, reads return the latest. */
export function fakeTeam(rec: TeamRecord = teamRecord()) {
  const state = { rec };
  vi.mocked(api.getTeam).mockImplementation(async () => state.rec);
  vi.mocked(api.saveTeam).mockImplementation(async (_id: string, revision: number, draft: TeamDraft) => {
    state.rec = { ...state.rec, draft: { ...draft, revision: revision + 1 } };
    return state.rec;
  });
  vi.mocked(api.listTeams).mockImplementation(async () => [
    { id: state.rec.draft.id, name: state.rec.draft.name, agent_count: state.rec.draft.agents.length, updated_at_ms: 1, started: null },
  ]);
  return state;
}

export function resetApi() {
  forgetBoardSelections();
  for (const f of Object.values(api)) if (vi.isMockFunction(f)) f.mockReset();
  vi.mocked(api.listRigs).mockResolvedValue([]);
  vi.mocked(api.listTeams).mockResolvedValue([]);
  vi.mocked(api.latestSeatEvents).mockResolvedValue([]);
  vi.mocked(api.templates).mockResolvedValue(templateInfos());
  vi.mocked(api.listFolders).mockImplementation(async (p?: string) => {
    if (!p || p === '/home/me') return HOME;
    if (p === '/home/me/projects') return PROJECTS;
    throw new ApiError(404, 'folder_missing', 'That folder does not exist');
  });
}

function Where() {
  const l = useLocation();
  return <div data-testid="where">{decodeURIComponent(l.pathname) + l.search}</div>;
}

export function renderAt(url: string) {
  return render(
    <MemoryRouter initialEntries={[url]}>
      <AppRoutes openStream={openFake} />
      <Where />
    </MemoryRouter>,
  );
}
export const where = () => screen.getByTestId('where').textContent;

/** The board's viewport has no size in jsdom: give it one so drops land on it. */
export function sizeViewport(w = 1000, h = 800) {
  const el = screen.getByTestId('structure-viewport');
  el.getBoundingClientRect = () => ({ left: 0, top: 0, right: w, bottom: h, width: w, height: h, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
  return el;
}

/** jsdom draws at the default viewport: world = client − 48 at scale 1. */
export const OFFSET = 48;

export const card = (name: string) => screen.getByRole('button', { name: new RegExp(`^${name} — `) });
export const slotOf = (name: string) => card(name).closest('.card-slot') as HTMLElement;
export const pos = (name: string) => [parseFloat(slotOf(name).style.left), parseFloat(slotOf(name).style.top)];
