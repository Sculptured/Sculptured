import { createContext, useContext, type FC } from 'react';
import type { PaneRef } from './model';
import type { PaneProps, PaneType, Registry } from './registry';
import { FollowContext } from './selection';

/**
 * A popped-out pane leaves a placeholder in its slot: a pane of type `popped:<its type>` whose
 * settings name the instance. It moves, tabs and closes like any pane.
 */
export const SLOT_PREFIX = 'popped:';

export const isSlot = (p: PaneRef) => p.type.startsWith(SLOT_PREFIX);
export const slotFor = (p: PaneRef): string | undefined => (isSlot(p) ? String(p.settings?.instance ?? '') : undefined);
export const slotRef = (id: string, pane: PaneRef): PaneRef => ({ id, type: SLOT_PREFIX + pane.type, settings: { instance: pane.id } });

export type SlotActions = { bringBack(instance: string): void; reopen(instance: string): void };
export const SlotContext = createContext<SlotActions | null>(null);

function PoppedSlot({ settings }: PaneProps) {
  const act = useContext(SlotContext);
  const instance = String(settings.instance ?? '');
  return (
    <div className="ws-popped">
      <p><span aria-hidden="true">⧉ </span>Shown in another window.</p>
      <div className="ws-popped-actions">
        <button type="button" className="btn btn-sm" onClick={() => act?.bringBack(instance)}>Bring back</button>
        <button type="button" className="btn btn-sm btn-ghost" onClick={() => act?.reopen(instance)}>Show that window</button>
      </div>
    </div>
  );
}

function FollowSetting({ settings, setSettings }: PaneProps) {
  return (
    <label className="ws-follow">
      <input type="checkbox" checked={settings.follow !== false} onChange={(e) => setSettings({ ...settings, follow: e.target.checked })} />
      Follow selection
    </label>
  );
}

/** A pane type that reads the selection gains `follow: true` by default and a "Follow selection" setting. */
function withFollow(t: PaneType): PaneType {
  const Inner = t.Body;
  const Own = t.Settings;
  const Body: FC<PaneProps> = (p) => (
    <FollowContext.Provider value={p.settings.follow !== false}><Inner {...p} /></FollowContext.Provider>
  );
  const Settings: FC<PaneProps> = (p) => (<><FollowSetting {...p} />{Own && <Own {...p} />}</>);
  return { ...t, defaultSettings: { follow: true, ...t.defaultSettings }, Body, Settings };
}

function slotType(real: PaneType | undefined): PaneType | undefined {
  return real && { type: SLOT_PREFIX + real.type, title: real.title, glyph: '⧉', Body: PoppedSlot };
}

/**
 * The page's registry as the shell uses it: selection readers wrapped for "Follow selection", and
 * placeholder types for popped-out panes. Types are made once each, so their components stay stable.
 */
export function shellRegistry(base: Registry): Registry {
  const made = new Map<string, PaneType | undefined>();
  const make = (type: string) => {
    if (type.startsWith(SLOT_PREFIX)) return slotType(base.get(type.slice(SLOT_PREFIX.length)));
    const t = base.get(type);
    return t?.selection ? withFollow(t) : t;
  };
  const get = (type: string): PaneType | undefined => {
    if (!made.has(type)) made.set(type, make(type));
    return made.get(type);
  };
  return { get, list: (ctx?: unknown) => base.list(ctx).map((t) => get(t.type)!) };
}
