import { useMemo, useSyncExternalStore } from 'react';
import { panes, setPaneSettings, stacks, type Node, type PaneRef } from './model';
import { PaneContext, type PaneType, type Registry } from './registry';
import { titleOf } from './StackView';

/** The app's mobile breakpoint (the stylesheets' `max-width: 720px`). */
const MOBILE = '(max-width: 720px)';
const query = () => globalThis.matchMedia?.(MOBILE);

function subscribe(change: () => void) {
  const q = query();
  q?.addEventListener?.('change', change);
  return () => q?.removeEventListener?.('change', change);
}

/** True below the mobile breakpoint, kept current as the window resizes. */
export function useMobile(): boolean {
  return useSyncExternalStore(subscribe, () => !!query()?.matches, () => false);
}

/** Option labels, numbered where two panes share a title. */
function labels(registry: Registry, list: PaneRef[]): string[] {
  const seen = new Map<string, number>();
  return list.map((p) => {
    const title = titleOf(registry, p);
    const n = (seen.get(title) ?? 0) + 1;
    seen.set(title, n);
    return n > 1 ? `${title} ${n}` : title;
  });
}

function OnePane({ pane, type, set }: { pane: PaneRef; type: PaneType | undefined; set(settings: Record<string, unknown>): void }) {
  const settings = useMemo(() => ({ ...type?.defaultSettings, ...pane.settings }), [type, pane.settings]);
  if (!type) return <p className="ws-empty">This kind of pane is no longer available.</p>;
  const Form = type.Settings;
  return (
    <>
      {Form && (
        <details className="ws-mobile-settings">
          <summary>Settings</summary>
          <Form instance={pane.id} settings={settings} setSettings={set} />
        </details>
      )}
      <div className="ws-mobile-body"><type.Body instance={pane.id} settings={settings} setSettings={set} /></div>
    </>
  );
}

type Props = { root: Node; registry: Registry; ctx: unknown; current: string | null; onCurrent(id: string): void; onChange(root: Node): void };

/** Below the mobile breakpoint: the layout's panes one at a time, chosen from a pane switcher. No dragging; the same panes. */
export function MobileWorkspace({ root, registry, ctx, current, onCurrent, onChange }: Props) {
  const list = panes(root);
  const first = stacks(root).find((s) => s.panes.length);
  const pane = list.find((p) => p.id === current) ?? first?.panes[first.active];
  const names = labels(registry, list);
  return (
    <PaneContext.Provider value={ctx}>
      <div className="ws-mobile">
        <label className="ws-mobile-switch">
          Pane
          <select value={pane?.id ?? ''} onChange={(e) => onCurrent(e.target.value)} disabled={!list.length}>
            {list.map((p, i) => <option key={p.id} value={p.id}>{names[i]}</option>)}
          </select>
        </label>
        {pane
          ? <OnePane key={pane.id} pane={pane} type={registry.get(pane.type)} set={(s) => onChange(setPaneSettings(root, pane.id, s))} />
          : <p className="ws-empty">Empty. Use Add pane.</p>}
      </div>
    </PaneContext.Provider>
  );
}
