import { useEffect, useState } from 'react';

/** Local display preferences only; never sent to the server. */
export const APPEARANCE_KEY = 'sculptured.appearance.v1';
/** Fired on window whenever this tab saves a preference (the palette and the dialog stay in step). */
export const APPEARANCE_CHANGED = 'sculptured:appearance-changed';
export const FONTS = {
  inter: { label: 'Inter', value: '"Inter", "Segoe UI", system-ui, sans-serif' },
  system: { label: 'System', value: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif' },
  plex: { label: 'IBM Plex Sans', value: '"IBM Plex Sans", system-ui, sans-serif' },
  humanist: { label: 'Humanist', value: '"Aptos", "Trebuchet MS", system-ui, sans-serif' },
} as const;
/** Looks (tokens.css). Coastal is dark only; Standard comes in light and dark. */
export const LOOKS = {
  coastal: { label: 'Coastal', note: 'Dark sea-glass.' },
  standard: { label: 'Standard', note: 'Neutral and flat, in light or dark.' },
} as const;
export const MODES = { system: 'System', light: 'Light', dark: 'Dark' } as const;
export type Look = keyof typeof LOOKS;
export type ColorMode = keyof typeof MODES;
export type Appearance = { font: keyof typeof FONTS; size: number; glass: boolean; look: Look; mode: ColorMode };
export const DEFAULT_APPEARANCE: Appearance = { font: 'inter', size: 13, glass: true, look: 'coastal', mode: 'system' };

export function readAppearance(): Appearance {
  try {
    const value = JSON.parse(localStorage.getItem(APPEARANCE_KEY) ?? 'null');
    if (!value || typeof value !== 'object') return { ...DEFAULT_APPEARANCE };
    return {
      font: Object.hasOwn(FONTS, value.font) ? value.font : DEFAULT_APPEARANCE.font,
      size: Number.isInteger(value.size) && value.size >= 12 && value.size <= 16 ? value.size : DEFAULT_APPEARANCE.size,
      glass: typeof value.glass === 'boolean' ? value.glass : DEFAULT_APPEARANCE.glass,
      look: Object.hasOwn(LOOKS, value.look) ? value.look : DEFAULT_APPEARANCE.look,
      mode: Object.hasOwn(MODES, value.mode) ? value.mode : DEFAULT_APPEARANCE.mode,
    };
  } catch {
    return { ...DEFAULT_APPEARANCE };
  }
}

/** The colour scheme the page shows: Coastal is always dark; Standard follows the mode or the system. */
export function resolvedScheme(value: Pick<Appearance, 'look' | 'mode'>): 'light' | 'dark' {
  if (value.look === 'coastal') return 'dark';
  if (value.mode !== 'system') return value.mode;
  return globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function applyAppearance(value: Appearance) {
  const root = document.documentElement;
  root.style.setProperty('--font-ui', FONTS[value.font].value);
  root.style.setProperty('--ui-size', `${value.size}px`);
  root.dataset.surface = value.glass ? 'glass' : 'solid';
  // Mirrors public/appearance-boot.js, which sets the same attributes before first paint.
  root.dataset.look = value.look;
  root.dataset.mode = value.mode;
  document.querySelector('meta[name="color-scheme"]')?.setAttribute('content', value.look === 'coastal' ? 'dark' : value.mode === 'system' ? 'light dark' : value.mode);
}

export function saveAppearance(value: Appearance) {
  applyAppearance(value);
  try { localStorage.setItem(APPEARANCE_KEY, JSON.stringify(value)); } catch { /* Still works for this session. */ }
  window.dispatchEvent(new CustomEvent<Appearance>(APPEARANCE_CHANGED, { detail: value }));
}

/** Change some preferences from outside the Appearance dialog (the command palette). */
export function updateAppearance(patch: Partial<Appearance>) {
  saveAppearance({ ...readAppearance(), ...patch });
}

/**
 * The look on screen, kept current when it changes here (Appearance, the palette) or in another tab.
 * For the few places where a look changes what is drawn, not only its tokens (Standard's header
 * status pill, footer usage strip and plain brand mark).
 */
export function useLook(): Look {
  const [look, setLook] = useState<Look>(() => readAppearance().look);
  useEffect(() => {
    const changed = (e: Event) => setLook((e as CustomEvent<Appearance>).detail?.look ?? readAppearance().look);
    const sync = (e: StorageEvent) => { if (e.key === APPEARANCE_KEY) setLook(readAppearance().look); };
    window.addEventListener(APPEARANCE_CHANGED, changed);
    window.addEventListener('storage', sync);
    return () => { window.removeEventListener(APPEARANCE_CHANGED, changed); window.removeEventListener('storage', sync); };
  }, []);
  return look;
}
