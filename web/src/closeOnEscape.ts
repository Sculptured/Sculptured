import type { KeyboardEvent } from 'react';

/** A keydown handler: Escape closes the panel and does not reach panels behind it. */
export const closeOnEscape = (onClose: () => void) => (e: KeyboardEvent) => {
  if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
};
