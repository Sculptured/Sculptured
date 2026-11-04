import { describe, expect, it } from 'vitest';
import { runStatus } from './runStatus';
import type { AgentLine } from './source';

const line = (status: AgentLine['status']): AgentLine => ({ id: status.word, name: status.word, status });

describe('runStatus', () => {
  it('reads the pause flag, never the status word', () => {
    const paused = line({ glyph: '‖', word: 'paused', tone: 'idle', pause: 'paused' });
    expect(runStatus({ agents: [paused], running: true }).word).toBe('paused');
    // A word that only looks like a pause (another source's own word) is not one.
    const lookalike = line({ glyph: '‖', word: 'paused', tone: 'idle' });
    expect(runStatus({ agents: [lookalike, line({ glyph: '●', word: 'running', tone: 'ok' })], running: true }).word).toBe('running');
    const pausing = line({ glyph: '⧗', word: 'pausing', tone: 'idle', pause: 'pausing' });
    expect(runStatus({ agents: [paused, pausing], running: true }).word).toBe('running');
    // Paused agents have no process: a fully paused team reads paused, not stopped.
    expect(runStatus({ agents: [paused, line({ glyph: '■', word: 'stopped', tone: 'idle' })], running: false }).word).toBe('paused');
  });
});
