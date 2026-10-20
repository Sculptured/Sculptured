import { render, type RenderResult } from '@testing-library/react';
import type { ReactElement } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { vi } from 'vitest';
import type { MeUser } from '../generated/community/MeUser';
import type { Profile } from '../generated/community/Profile';
import type { Publication } from '../generated/community/Publication';
import { CommunityContext, type CommunityState } from './context';

/** `ui` at `path`, inside a router and a Community state (signed out, hosted, by default). */
export function renderCommunity(ui: ReactElement, o: { path: string; route?: string; state?: Partial<CommunityState> }): RenderResult {
  const state: CommunityState = { signedIn: false, workspace: o.state?.signedIn ?? false, me: null, meState: 'ready', meError: null, reloadMe: async () => {}, ...o.state };
  return render(
    <MemoryRouter initialEntries={[o.path]}>
      <CommunityContext.Provider value={state}>
        <Routes><Route path={o.route ?? o.path.split('?')[0]} element={ui} /></Routes>
      </CommunityContext.Provider>
    </MemoryRouter>,
  );
}

/** Answers fetches by "METHOD /url"; anything else is a 404 not_found. Records every call. */
export function fakeFetch(routes: Record<string, (init?: RequestInit) => [number, unknown] | Response>): { calls: string[]; inits: (RequestInit | undefined)[] } {
  const calls: string[] = [];
  const inits: (RequestInit | undefined)[] = [];
  vi.spyOn(globalThis, 'fetch').mockImplementation(async (url, init) => {
    const key = `${init?.method ?? 'GET'} ${String(url)}`;
    calls.push(key);
    inits.push(init);
    const route = routes[key];
    const out = route ? route(init) : ([404, { error: { code: 'not_found', message: 'Not found' } }] as [number, unknown]);
    if (out instanceof Response) return out;
    return new Response(JSON.stringify(out[1]), { status: out[0], headers: { 'content-type': 'application/json' } });
  });
  return { calls, inits };
}

const avatar = 'data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=';

export function publication(o: Partial<Publication> = {}): Publication {
  return {
    kind: 'group', id: 'b1', author: { handle: 'ana', name: 'Ana Lee', avatar_url: avatar }, version: 1, title: 'Review pair',
    description: 'A writer and a reviewer.', license: 'MIT', tags: ['review'], digest: 'd',
    content: {
      id: 'review-pair', name: 'Review pair', revision: 0, boundary_notes: [],
      draft: {
        id: 'review-pair', name: 'Review pair', revision: 0, connections: [],
        groups: [{ id: 'g', label: 'Review pair', x: 0, y: 0, w: 300, h: 200 }],
        agents: [{ id: 'writer', group_id: 'g', name: 'Writer', kind: 'claude', job: 'Write it.', folder: null, x: 20, y: 20, advanced: { model: null, permission: 'restricted', resume: 'relaunch-fresh' } }],
      },
    },
    published: true, ...o,
  } as Publication;
}

export function profile(o: Partial<Profile> = {}): Profile {
  return { handle: 'ana', name: 'Ana Lee', bio: '', links: [], avatar_url: avatar, joined_at: '2026-10-01T00:00:00Z', counts: { published: 1, saved_by_others: 0 }, ...o };
}

export function meUser(o: Partial<MeUser> = {}): MeUser {
  return { handle: 'ana', name: 'Ana Lee', bio: '', links: [], avatar_url: avatar, admin: false, workspace: true, settings: { compact: true }, ...o };
}
