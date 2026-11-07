import type { IconName } from '../shell/ui';
import type { PhaseStatus, Step } from './process';

export const STEP_WORD: Record<Step['status'], string> = { ok: '✓ ok', error: '✗ error', running: 'running' };

export const PHASE_WORD: Record<PhaseStatus, string> = {
  completed: '✓ completed',
  failed: '✗ failed',
  interrupted: '◼ interrupted',
  running: 'running',
  'cut-off': '✗ cut off: the session ended',
};

export const ACTION_ICON: Record<string, IconName> = { read: 'file-text', edited: 'pencil', wrote: 'file-plus', created: 'file-plus' };

/** A one-line summary of Markdown: heading, quote and emphasis markers dropped, words kept. */
export function plainLine(s: string): string {
  return s
    .replace(/^\s{0,3}(#{1,6}\s+|>\s*|[-*+]\s+|\d+[.)]\s+)/, '')
    .replace(/(\*\*|__)(.+?)\1/g, '$2')
    .replace(/(^|[^\w*])[*_](\S(?:.*?\S)?)[*_](?=[^\w*]|$)/g, '$1$2')
    .replace(/`([^`]+)`/g, '$1');
}

export function Running() {
  return <span className="spin-ring" aria-hidden="true" />;
}

