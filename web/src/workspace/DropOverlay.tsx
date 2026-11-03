import type { Zone } from './model';

const WHERE: Record<Zone, string> = {
  left: 'Drop to put it on the left',
  right: 'Drop to put it on the right',
  top: 'Drop to put it above',
  bottom: 'Drop to put it below',
  center: 'Drop to add it as a tab',
};

/** Shows, over the stack under the pointer, where a dragged pane would land. */
export function DropOverlay({ zone }: { zone: Zone }) {
  return (
    <div className={`ws-drop ws-drop-${zone}`} data-drop-zone={zone} aria-hidden="true">
      <span className="ws-drop-label">{WHERE[zone]}</span>
    </div>
  );
}
