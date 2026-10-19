import { expect, it } from 'vitest';
import { communityFor } from './link';

it('the hosted app signs in to Community with the Sculptured account; the self-run app has no Community yet', () => {
  expect(communityFor({ kind: 'signed-in', user: { id: 'u', name: 'Ana', email: 'a@x', role: 'member' }, csrf: 't' })).toEqual({ available: true, signedIn: true });
  expect(communityFor({ kind: 'signed-out' })).toEqual({ available: true, signedIn: false });
  expect(communityFor({ kind: 'local' })).toEqual({ available: false, signedIn: false });
});
