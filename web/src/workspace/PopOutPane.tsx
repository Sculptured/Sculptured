import { useEffect, useMemo, useState } from 'react';
import { panes, type PageLayouts, type PaneRef } from './model';
import { PaneContext, type PaneType, type Registry } from './registry';
import { SelectionProvider, useChannelPost } from './selection';
import { isSlot, shellRegistry } from './slots';
import { loadLayouts } from './store';

type Props = { page: string; registry: Registry; ctx: unknown; instance: string };

const NONE = (): PageLayouts => ({ version: 1, active: '', layouts: [] });

/** The pane as saved by the main window: popped out first, else still in a layout. */
function findPane(page: string, registry: Registry, instance: string): { pane?: PaneRef; look?: PageLayouts['look'] } {
  const value = loadLayouts(page, NONE, (t) => !!registry.get(t));
  const popped = value.layouts.flatMap((l) => l.popped);
  const placed = value.layouts.flatMap((l) => panes(l.root)).filter((p) => !isSlot(p));
  return { pane: [...popped, ...placed].find((p) => p.id === instance), look: value.look };
}

/** Settings live here while the window is open, and are sent to the main window to keep. */
function useSettings(pane: PaneRef, type: PaneType) {
  const post = useChannelPost();
  const [own, setOwn] = useState(pane.settings);
  const settings = useMemo(() => ({ ...type.defaultSettings, ...own }), [type, own]);
  const setSettings = (s: Record<string, unknown>) => { setOwn(s); post({ kind: 'settings', instance: pane.id, settings: s }); };
  return { settings, setSettings };
}

/** Tells the main window this pane is coming back when the window closes. */
function useBackOnClose(instance: string) {
  const post = useChannelPost();
  useEffect(() => {
    const back = () => post({ kind: 'back', instance });
    window.addEventListener('pagehide', back);
    return () => window.removeEventListener('pagehide', back);
  }, [post, instance]);
}

function Popped({ pane, type }: { pane: PaneRef; type: PaneType }) {
  const { settings, setSettings } = useSettings(pane, type);
  const [editing, setEditing] = useState(false);
  useBackOnClose(pane.id);
  useEffect(() => { document.title = `${type.title} · Sculptured`; }, [type.title]);
  const Form = type.Settings;
  return (
    <>
      <header className="workspace-popout-head">
        <span className="ws-glyph" aria-hidden="true">{type.glyph}</span>
        <h1>{type.title}</h1>
        {Form && <button type="button" className="btn btn-sm btn-ghost" aria-expanded={editing} onClick={() => setEditing((e) => !e)}>Settings</button>}
      </header>
      {Form && editing && <div className="ws-settings"><Form instance={pane.id} settings={settings} setSettings={setSettings} /></div>}
      <div className="workspace-popout-body"><type.Body instance={pane.id} settings={settings} setSettings={setSettings} /></div>
    </>
  );
}

/**
 * A popped-out pane on its own, at the page's pane route. It shares the workspace's selection, and
 * closing the window puts the pane back in the main window's layout.
 */
export function PopOutPane({ page, registry, ctx, instance }: Props) {
  const shell = useMemo(() => shellRegistry(registry), [registry]);
  const [{ pane, look }] = useState(() => findPane(page, shell, instance));
  const type = pane && shell.get(pane.type);
  return (
    <SelectionProvider page={page}>
      <PaneContext.Provider value={ctx}>
        <div className="workspace workspace-popout" data-look={look === 'app' ? undefined : 'mission'}>
          {pane && type
            ? <Popped pane={pane} type={type} />
            : <p className="ws-empty">This pane is no longer in the workspace. Close this window and use Add pane in the main window.</p>}
        </div>
      </PaneContext.Provider>
    </SelectionProvider>
  );
}
