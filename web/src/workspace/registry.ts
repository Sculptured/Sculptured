import { createContext, useContext, type FC } from 'react';

/** What every pane body and settings form receives: its own instance id and settings. */
export type PaneProps<S = Record<string, unknown>> = { instance: string; settings: S; setSettings(s: S): void };

export type PaneType = {
  type: string;
  title: string;
  glyph: string;
  /** Several copies may be open at once (duplicate, or "Add pane" again). */
  multi?: boolean;
  /** Page-specific context; without it the pane applies everywhere. */
  appliesTo?: (ctx: unknown) => boolean;
  defaultSettings?: Record<string, unknown>;
  Settings?: FC<PaneProps>;
  Body: FC<PaneProps>;
};

export function createRegistry(types: PaneType[]) {
  const byType = new Map(types.map((t) => [t.type, t]));
  return {
    get: (type: string): PaneType | undefined => byType.get(type),
    list: (ctx?: unknown): PaneType[] => types.filter((t) => !t.appliesTo || t.appliesTo(ctx)),
  };
}

export type Registry = ReturnType<typeof createRegistry>;

/** The page's live context (selection, deployment kind…), for pane bodies that need it. */
export const PaneContext = createContext<unknown>(undefined);
export const usePaneContext = <T = unknown>() => useContext(PaneContext) as T;
