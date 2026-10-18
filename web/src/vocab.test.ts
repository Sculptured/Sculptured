import { describe, expect, it } from 'vitest';
import { displayName, edgeSentence, labelNames, nameForId, RUNTIME_LABEL, WORDS } from './vocab';
import { compiledRig, seatView } from './test-fixtures';

describe('vocab', () => {
  it('names an agent by the team name, then its label, then its short id', () => {
    expect(displayName(seatView('a'))).toBe('a');
    expect(displayName(seatView('a', { label: 'Builder' }))).toBe('Builder');
    expect(displayName(seatView('a', { label: '  ' }))).toBe('a');
    expect(displayName(seatView('a', { label: 'Builder' }), { 'dev.a@r': 'Lead builder' })).toBe('Lead builder');
    expect(nameForId('dev.b@r')).toBe('b');
    expect(nameForId('dev.b@r', { 'dev.b@r': 'Reviewer' })).toBe('Reviewer');
  });

  it('says connections in words and marks design intent as not enforced', () => {
    const [enforced, intent] = compiledRig().edges;
    expect(edgeSentence(enforced)).toBe('a hands work to b');
    expect(edgeSentence(intent)).toBe('b watches a (not enforced)');
    expect(edgeSentence(enforced, { 'dev.a@r': 'Builder', 'dev.b@r': 'Reviewer' })).toBe('Builder hands work to Reviewer');
  });

  it('uses labels from the compiled team', () => {
    const rig = compiledRig();
    rig.seats[0].label = 'Builder';
    expect(labelNames(rig)).toEqual({ 'dev.a@r': 'Builder' });
  });

  it('calls the stub a Practice agent and counts in words', () => {
    expect(RUNTIME_LABEL.stub).toBe('Practice agent');
    expect(WORDS.teamCounts(1, 3)).toBe('1 of 3 agents running');
    expect(WORDS.needAttention(1)).toBe('1 needs attention');
    expect(WORDS.needAttention(2)).toBe('2 need attention');
    expect(WORDS.session(0)).toBe('not started yet');
    expect(WORDS.session(2)).toBe('session #2');
  });
});
