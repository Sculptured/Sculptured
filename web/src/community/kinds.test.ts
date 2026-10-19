import { expect, it } from 'vitest';
import { opensHere } from './kinds';

it('treats a missing kind as a group and anything else as not openable here', () => {
  expect(opensHere({ id: 'x' })).toBe(true);
  expect(opensHere({ id: 'x', kind: 'group' })).toBe(true);
  expect(opensHere({ id: 'x', kind: 'component' })).toBe(false);
});
