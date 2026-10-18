import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { APPEARANCE_CHANGED, APPEARANCE_KEY, DEFAULT_APPEARANCE, FONTS, LOOKS, MODES, applyAppearance, readAppearance, saveAppearance, type Appearance, type ColorMode, type Look } from '../appearance';
import { Btn } from './ui';

const OPEN_EVENT = 'sculptured:appearance';
/** Opens the Appearance dialog from elsewhere (the command palette). */
export const openAppearance = () => window.dispatchEvent(new Event(OPEN_EVENT));

/** The Appearance dialog, and its "Aa" button unless `trigger` is false (the app menu opens it then). */
export function AppearanceControl({ trigger: showTrigger = true }: { trigger?: boolean } = {}) {
  const [value, setValue] = useState(readAppearance);
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const close = () => { setOpen(false); trigger.current?.focus(); };
  const update = (patch: Partial<Appearance>) => {
    const next = { ...value, ...patch };
    setValue(next);
    saveAppearance(next);
  };
  useEffect(() => {
    applyAppearance(value);
  }, [value]);
  useEffect(() => {
    const show = () => setOpen(true);
    window.addEventListener(OPEN_EVENT, show);
    return () => window.removeEventListener(OPEN_EVENT, show);
  }, []);
  useEffect(() => {
    const sync = (e: StorageEvent) => { if (e.key === APPEARANCE_KEY || e.key === null) setValue(readAppearance()); };
    // The command palette saves through appearance.ts; follow it so the dialog never shows a stale choice.
    const changed = (e: Event) => { const next = (e as CustomEvent<Appearance>).detail; if (next) setValue(next); };
    window.addEventListener('storage', sync);
    window.addEventListener(APPEARANCE_CHANGED, changed);
    return () => { window.removeEventListener('storage', sync); window.removeEventListener(APPEARANCE_CHANGED, changed); };
  }, []);
  useEffect(() => {
    if (open) dialog.current?.querySelector<HTMLButtonElement>('button')?.focus();
  }, [open]);
  return <>
    {showTrigger && <Btn ref={trigger} variant="ghost" className="appearance-trigger" aria-label="Appearance" aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(true)} tip="Look, font and text size">
      <span aria-hidden="true">Aa</span>
    </Btn>}
    {open && createPortal(
      <div className="overlay appearance-overlay" onMouseDown={e => { if (e.target === e.currentTarget) close(); }}>
        <div className="panel appearance-dialog" ref={dialog} role="dialog" aria-modal="true" aria-labelledby="appearance-title" onKeyDown={e => {
          // Keep app-wide navigation and execution shortcuts out of this dialog.
          if (e.key !== 'Tab') e.stopPropagation();
          if (e.key === 'Escape') { e.preventDefault(); close(); }
          if (e.key === 'Tab') {
            const controls = [...dialog.current!.querySelectorAll<HTMLElement>('*')].filter(el => el.matches('button, select, input') && !el.hasAttribute('disabled'));
            const first = controls[0], last = controls[controls.length - 1];
            if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
            else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
          }
        }}>
          <div className="panel-bar"><h2 id="appearance-title" className="panel-title">Appearance</h2><span className="panel-rule" /><Btn variant="ghost" icon="x" iconOnly aria-label="Close appearance" onClick={close} /></div>
          <AppearanceBody value={value} update={update} />
          <div className="appearance-foot"><Btn variant="ghost" onClick={() => update(DEFAULT_APPEARANCE)}>Reset defaults</Btn><Btn variant="primary" onClick={close}>Done</Btn></div>
        </div>
      </div>, document.body)}
  </>;
}

/** The dialog's fields: look (and Standard's mode), font, text size and Coastal's translucency. */
function AppearanceBody({ value, update }: { value: Appearance; update: (patch: Partial<Appearance>) => void }) {
  return (
    <div className="appearance-body">
      <p className="muted">Make this workspace comfortable to read.</p>
      <div className="appearance-field"><span id="appearance-look">Look</span><div className="seg appearance-seg" role="group" aria-labelledby="appearance-look">{(Object.keys(LOOKS) as Look[]).map(id => <Btn key={id} pressed={value.look === id} onClick={() => update({ look: id })}>{LOOKS[id].label}</Btn>)}</div><span className="muted appearance-hint">{LOOKS[value.look].note}</span></div>
      {value.look === 'standard' && <div className="appearance-field"><span id="appearance-mode">Mode</span><div className="seg appearance-seg" role="group" aria-labelledby="appearance-mode">{(Object.keys(MODES) as ColorMode[]).map(id => <Btn key={id} pressed={value.mode === id} onClick={() => update({ mode: id })}>{MODES[id]}</Btn>)}</div><span className="muted appearance-hint">System follows your computer's light or dark setting.</span></div>}
      <label className="appearance-field">Interface font<select value={value.font} onChange={e => update({ font: e.target.value as Appearance['font'] })}>{Object.entries(FONTS).map(([id, font]) => <option key={id} value={id}>{font.label}</option>)}</select></label>
      <div className="appearance-field"><label htmlFor="text-size">Text size <output htmlFor="text-size">{value.size} px</output></label><div className="appearance-size"><Btn aria-label="Smaller text" disabled={value.size === 12} onClick={() => update({ size: value.size - 1 })}>A−</Btn><input id="text-size" type="range" min="12" max="16" step="1" value={value.size} onChange={e => update({ size: Number(e.target.value) })} /><Btn aria-label="Larger text" disabled={value.size === 16} onClick={() => update({ size: value.size + 1 })}>A+</Btn></div><span className="muted appearance-hint">Compact to large. Code stays in a monospace font.</span></div>
      {value.look === 'coastal' && <label className="appearance-switch"><input type="checkbox" checked={value.glass} onChange={e => update({ glass: e.target.checked })} /><span>Translucent surfaces<small>Turn off for a solid, quieter workspace.</small></span></label>}
      <div className="appearance-sample"><strong>Source & chapter converter</strong><p>Waiting for Chapter designer to finish.</p><code>src/components/Workspace.tsx</code></div>
      <p className="appearance-hint muted">Saved in this browser. Your system’s reduced transparency setting takes priority.</p>
    </div>
  );
}
