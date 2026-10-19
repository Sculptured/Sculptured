import { afterEach, expect, it, vi } from 'vitest';
import { libraryCopy, libraryState, saveToLibrary } from './saveToLibrary';
import { fakeFetch, publication } from './testing';

afterEach(() => vi.restoreAllMocks());

it('saves once, then knows it is in the Library, and offers Update to v2 when Community has a newer version', async () => {
  const { calls, inits } = fakeFetch({
    'GET /api/group-blocks': () => [200, []],
    'PUT /api/group-blocks/community-b1': (init) => [200, { ...JSON.parse(String(init!.body)).block, revision: 1 }],
  });
  const copyV1 = await saveToLibrary(publication({ id: 'b1', version: 1 }));
  expect(calls).toEqual(['GET /api/group-blocks', 'PUT /api/group-blocks/community-b1']);
  const sent = JSON.parse(String(inits[1]!.body)).block;
  expect(sent.revision).toBe(0);
  expect(sent.source).toEqual({ community_block: 'b1', version: 1 });
  expect(libraryState(publication({ id: 'b1', version: 1 }), [copyV1]).kind).toBe('same');
  expect(libraryState(publication({ id: 'b1', version: 2 }), [copyV1])).toMatchObject({ kind: 'older', has: 1 });
  expect(libraryState(publication({ id: 'other' }), [copyV1]).kind).toBe('none');
  expect(libraryCopy(publication({ id: 'b1', version: 2 }), copyV1)).toMatchObject({ id: copyV1.id, revision: copyV1.revision, source: { version: 2 } });
});
