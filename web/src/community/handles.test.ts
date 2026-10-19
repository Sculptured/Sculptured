import { expect, it } from 'vitest';
import { COMMUNITY_ERROR_WORDS } from '../errors';
import { handleProblem, RESERVED_HANDLES, suggestHandle } from './handles';

it('follows Community’s handle rules', () => {
  for (const ok of ['abc', 'a-b-c', 'a'.repeat(30)]) expect(handleProblem(ok)).toBeNull();
  for (const bad of ['ab', 'a'.repeat(31), '-ab', 'ab-', 'A-b', 'a_b']) expect(handleProblem(bad)).toBe(COMMUNITY_ERROR_WORDS.handle_invalid);
  for (const r of RESERVED_HANDLES) expect(handleProblem(r)).toBe(COMMUNITY_ERROR_WORDS.handle_reserved);
  expect(RESERVED_HANDLES).toHaveLength(16);
  for (const name of ['Ana Lee!', 'José Núñez', '李明', 'x', 'Admin', 'a'.repeat(50)]) expect(handleProblem(suggestHandle(name)), name).toBeNull();
  expect(suggestHandle('Ana Lee!')).toBe('ana-lee');
});
