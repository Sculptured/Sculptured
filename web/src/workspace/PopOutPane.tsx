import { useEffect, useMemo, useRef, useState } from 'react';
import { panes, type PageLayouts, type PaneRef } from './model';
import { PaneContext, type PaneType, type Registry } from './registry';
import { SelectionProvider, useChannelListener, useChannelPost } from './selection';
import { isSlot, shellRegistry } from './slots';
import { loadLayouts } from './store';

/** `channel`: the selection channel when it is not the page's (see WorkspacePage). */
type Props = { page: string; channel?: string; registry: Registry; ctx: unknown; instance: string };

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

/**
 * This window's part in who shows the pane. On mount it says "out", so the main window moves the
 * pane to `popped` even after a reload. When the page goes it says "back". When the main window
 * says "close" (it took the pane back), it closes, or says so if the browser keeps it open.
 */
function useShownHere(instance: string): boolean {
  const post = useChannelPost();
  const [gone, setGone] = useState(false);
  const left = useRef(false);
  useEffect(() => post({ kind: 'out', instance }), [post, instance]);
  useChannelListener((m) => {
    if (m.kind !== 'close' || m.instance !== instance) return;
    left.current = true;
    setGone(true);
    window.close();
  });
  useEffect(() => {
    const back = () => { if (!left.current) post({ kind: 'back', instance }); };
    window.addEventListener('pagehide', back);
    return () => window.removeEventListener('pagehide', back);
  }, [post, instance]);
  return gone;
}

function ShownHere({ pane, type }: { pane: PaneRef; type: PaneType }) {
  const gone = useShownHere(pane.id);
  if (gone) return <p className="ws-empty">This pane is back in the main window. You can close this window.</p>;
  return <Popped pane={pane} type={type} />;
}

function Popped({ pane, type }: { pane: PaneRef; type: PaneType }) {
  const { settings, setSettings } = useSettings(pane, type);
  const [editing, setEditing] = useState(false);
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
export function PopOutPane({ page, channel, registry, ctx, instance }: Props) {
  const shell = useMemo(() => shellRegistry(registry), [registry]);
  const [{ pane, look }] = useState(() => findPane(page, shell, instance));
  const type = pane && shell.get(pane.type);
  return (
    <SelectionProvider page={channel ?? page}>
      <PaneContext.Provider value={ctx}>
        <div className="workspace workspace-popout" data-look={look === 'app' ? undefined : 'mission'}>
          {pane && type
            ? <ShownHere pane={pane} type={type} />
            : <p className="ws-empty">This pane is no longer in the workspace. Close this window and use Add pane in the main window.</p>}
        </div>
      </PaneContext.Provider>
    </SelectionProvider>
  );
}
