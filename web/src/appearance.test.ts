import { beforeEach, describe, expect, it } from 'vitest';
import { APPEARANCE_KEY, LOOKS, applyAppearance, readAppearance, resolvedScheme, saveAppearance } from './appearance';

beforeEach(() => localStorage.clear());

describe('appearance', () => {
  it('appearance_accepts_mission', () => {
    expect(Object.keys(LOOKS)).toEqual(['coastal', 'standard', 'mission']);
    localStorage.setItem(APPEARANCE_KEY, JSON.stringify({ look: 'mission', mode: 'light' }));
    const value = readAppearance();
    expect(value.look).toBe('mission');
    // Mission is dark only, whatever the mode says.
    expect(resolvedScheme(value)).toBe('dark');
    const meta = document.head.appendChild(document.createElement('meta'));
    meta.name = 'color-scheme';
    applyAppearance(value);
    expect(document.documentElement.dataset.look).toBe('mission');
    expect(meta.content).toBe('dark');
    saveAppearance({ ...value, look: 'standard' });
    expect(readAppearance().look).toBe('standard');
    localStorage.setItem(APPEARANCE_KEY, JSON.stringify({ look: 'bridge' }));
    expect(readAppearance().look).toBe('coastal');
    meta.remove();
  });
});
