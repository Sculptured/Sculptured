import { describe, expect, it } from 'vitest';
import { fuzzyScore } from './fuzzy';

describe('fuzzyScore', () => {
  it('matches whole words, then letters in order, and nothing else', () => {
    expect(fuzzyScore('group', 'Add a group')).not.toBeNull();
    expect(fuzzyScore('adgr', 'Add a group')).not.toBeNull();
    expect(fuzzyScore('gx', 'Add a group')).toBeNull();
    // Every term must match.
    expect(fuzzyScore('add zebra', 'Add a group')).toBeNull();
  });

  it('ranks a whole word at a word start above scattered letters', () => {
    expect(fuzzyScore('stop', 'Stop team…')!).toBeGreaterThan(fuzzyScore('stop', 'Set the top')!);
    expect(fuzzyScore('start', 'Start team…')!).toBeGreaterThan(fuzzyScore('start', 'Restart the agent')!);
  });
});
