import { expect, it } from 'vitest';
import { buildTeams } from './model';
import { snapshot } from '../test-fixtures';

it('opens every team on its landing page, Control (Ruling 19)', () => {
  const teams = (desired: 'running' | 'stopped') => ({ r: { state: { snapshot: snapshot({ desired }), seats: {}, feed: {}, epoch: 0 }, connected: true } }) as never;
  expect(buildTeams(teams('running'), ['r'], {})[0].href).toBe('/p/r');
  expect(buildTeams(teams('stopped'), ['r'], {})[0].href).toBe('/p/r');
  expect(buildTeams({} as never, ['never-started'], {})[0].href).toBe('/p/never-started');
});
