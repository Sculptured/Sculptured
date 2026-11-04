import { describe, expect, it } from 'vitest';
import type { Toast } from './Toasts';
import { stillTrue } from './useToasts';

const toast = (agent: string, kind: Toast['kind']): Toast => ({ id: 1, agent, rig: 'r', name: agent, kind, team: 'T' });
const rows = (key: string) => [{ agents: [{ id: 'a', state: { key } }] }] as never;

describe('stillTrue', () => {
  it('drops a "needs you" once the agent no longer needs you, and keeps the rest', () => {
    expect(stillTrue([toast('a', 'needs-you')], rows('needs-you'))).toHaveLength(1);
    expect(stillTrue([toast('a', 'needs-you')], rows('working'))).toHaveLength(0);
    expect(stillTrue([toast('a', 'done')], rows('working'))).toHaveLength(1);
    // An agent no team lists now (still loading) keeps its toast.
    expect(stillTrue([toast('b', 'needs-you')], rows('working'))).toHaveLength(1);
  });
});
