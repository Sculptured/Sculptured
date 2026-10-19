import { afterEach, expect, it, vi } from 'vitest';
import { setCsrf } from '../api';
import { communityApi, safeAvatarUrl } from './api';
import { fakeFetch } from './testing';

afterEach(() => { vi.restoreAllMocks(); setCsrf(null); });

it('calls only relative /api/community URLs and sends PATCH, a picture and the CSRF token', async () => {
  const { calls, inits } = fakeFetch({ 'PATCH /api/community/me': () => [200, { user: null }], 'PUT /api/community/me/avatar': () => [200, { avatar_url: '/api/community/users/a/avatar' }] });
  setCsrf('t');
  await communityApi.updateMe({ bio: 'hi' });
  const pic = new Blob([new Uint8Array(3)], { type: 'image/png' });
  await communityApi.putAvatar(pic);
  expect(calls).toEqual(['PATCH /api/community/me', 'PUT /api/community/me/avatar']);
  const h = inits[1]!.headers as Record<string, string>;
  expect(h['x-csrf-token']).toBe('t');
  expect(h['content-type']).toBe('image/png');
  expect(inits[1]!.body).toBe(pic);
});

it('encodes handles and ids in paths', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/users/a%20b': () => [200, { profile: {}, blocks: [] }], 'GET /api/community/blocks/x%2Fy': () => [200, {}] });
  await communityApi.profile('a b');
  await communityApi.block('x/y');
  expect(calls).toEqual(['GET /api/community/users/a%20b', 'GET /api/community/blocks/x%2Fy']);
});

it('keeps only same-origin avatar pictures and initials', () => {
  expect(safeAvatarUrl('/api/community/users/ana/avatar?v=1')).toBe('/api/community/users/ana/avatar?v=1');
  expect(safeAvatarUrl('data:image/svg+xml;base64,PHN2Zz4=')).toBe('data:image/svg+xml;base64,PHN2Zz4=');
  for (const u of ['https://app.x/api/community/users/a/avatar', '//evil/x', 'javascript:alert(1)', 'data:text/html,x', '/api/teams', null]) expect(safeAvatarUrl(u)).toBeNull();
});

it('an old-style {"error":"text"} answer is community_bad_answer and its text is never shown', async () => {
  fakeFetch({ 'GET /api/community/blocks?q=&offset=0': () => [400, { error: 'Invalid search' }] });
  await expect(communityApi.blocks('', 0)).rejects.toMatchObject({ code: 'community_bad_answer', message: '' });
});
