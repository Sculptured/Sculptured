import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { routeTemplate, startAnalytics, track } from './analytics';

let posts: { events: { name: string; props?: Record<string, unknown> }[] }[] = [];
let stop: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  posts = [];
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init: RequestInit) => {
    posts.push(JSON.parse(String(init.body)));
    return new Response(null, { status: 204 });
  }));
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
});

afterEach(() => {
  stop?.();
  stop = null;
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const sent = () => posts.flatMap((p) => p.events);

it('reports the session and active time per page template while the tab is in front', async () => {
  stop = startAnalytics('csrf-1', () => '/team/:id/structure');
  await vi.advanceTimersByTimeAsync(30_000);
  const events = sent();
  expect(events[0].name).toBe('session.start');
  const active = events.filter((e) => e.name === 'page.active');
  expect(active).toHaveLength(2);
  expect(active[0].props).toEqual({ route: '/team/:id/structure', seconds: 15 });
  const init = vi.mocked(fetch).mock.calls[0][1] as RequestInit;
  expect((init.headers as Record<string, string>)['x-csrf-token']).toBe('csrf-1');

  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
  const before = sent().filter((e) => e.name === 'page.active').length;
  await vi.advanceTimersByTimeAsync(30_000);
  expect(sent().filter((e) => e.name === 'page.active').length).toBe(before);
});

it('track is a no-op until started (local mode never starts it)', async () => {
  track('describe-it');
  await vi.advanceTimersByTimeAsync(30_000);
  expect(fetch).not.toHaveBeenCalled();
  stop = startAnalytics('c', () => '/');
  track('describe-it');
  await vi.advanceTimersByTimeAsync(15_000);
  expect(sent().some((e) => e.name === 'ui.action' && e.props?.name === 'describe-it')).toBe(true);
});

it('routes keep only fixed words, never names', () => {
  expect(routeTemplate('/team/secret-plan/structure')).toBe('/team/:id/structure');
  expect(routeTemplate('/rig/my-team/agent/team.a@my-team/conversation')).toBe('/rig/:id/agent/:id/:id');
  expect(routeTemplate('/library/groups')).toBe('/library/:id');
  expect(routeTemplate('/')).toBe('/');
  expect(routeTemplate('/team/textbook/structure')).toBe('/team/:id/structure');
  expect(routeTemplate('/rig/library/agent/team.a@library/git')).toBe('/rig/:id/agent/:id/:id');
  expect(routeTemplate('/community/some-author/some-block')).toBe('/community/:id/:id');
});

it('a team drafted with Describe it is not counted as a file import', async () => {
  const { api } = await import('../api');
  stop = startAnalytics('c', () => '/');
  vi.mocked(fetch).mockClear();
  vi.mocked(fetch).mockImplementation(async (_u: string | URL | Request, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body ?? '{}'));
    if (body.events) posts.push(body);
    return new Response(JSON.stringify({ team: { draft: { id: 't' } }, warnings: [] }), { status: 200 });
  });
  await api.createDraft({} as never, 'r1', { kind: 'generated', cli: 'claude', model: null, at_ms: 1, prompt: 'p' });
  await api.importMadl('madl: draft', 'r2');
  await vi.advanceTimersByTimeAsync(15_000);
  expect(sent().filter((e) => e.name === 'ui.action' && e.props?.name === 'import-team-file')).toHaveLength(1);
});
