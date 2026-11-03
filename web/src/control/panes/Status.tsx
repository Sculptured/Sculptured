import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { afterName, alarmIdOf, clockAt, nameIn, useControl } from '../context';
import type { FeedItem } from '../source';

/** The Status pane: what is happening, in plain words, newest first; alarms are marked. */

/** Event kinds in groups the Settings can hide, by the record's own type. Anything else is "other". */
const GROUPS: { id: string; label: string; kinds: RegExp }[] = [
  { id: 'status', label: 'Status changes', kinds: /^(seat-|tenure-|rig-|team-|restore-|new-session|features-|model-|stall-|context-ceiling|limit-|budget-|events-lost)/ },
  { id: 'turns', label: 'Turns and replies', kinds: /^(turn-|agent-text|thinking-recorded)/ },
  { id: 'tools', label: 'Tool use', kinds: /^(tool-|protection-applied|action-interpreted)/ },
  { id: 'messages', label: 'Messages and handoffs', kinds: /^(message-|handoff-|handed-over|handover-|trigger-)/ },
  { id: 'questions', label: 'Questions and changes', kinds: /^(question-|change-)/ },
  { id: 'results', label: 'Reports and checks', kinds: /^(findings-reported|status-page-written|result-checked|check-|report-rejected)/ },
  { id: 'usage', label: 'Token reports', kinds: /^(usage-reported|context-reported|plan-usage-reported)$/ },
];
const OTHER = { id: 'other', label: 'Other' };
export const KIND_GROUPS = [...GROUPS.map(({ id, label }) => ({ id, label })), OTHER];

export function groupOf(kind: string): string {
  return GROUPS.find((g) => g.kinds.test(kind))?.id ?? OTHER.id;
}

/** Rows kept on screen; older ones are in Raw events. */
const SHOWN = 400;

export type StatusSettings = { hidden?: string[]; onlyAlarms?: boolean; onlySelected?: boolean };
export const STATUS_DEFAULTS: StatusSettings = { hidden: ['usage'], onlyAlarms: false, onlySelected: false };

function Row({ item, mark, picked }: { item: FeedItem; mark: string | null; picked: boolean }) {
  const { source } = useControl();
  const { select } = useSelection();
  const who = nameIn(source, item.agent);
  return (
    <li className={`ctl-feed-row${mark ? ' is-alarm' : ''}${picked ? ' is-picked' : ''}`}>
      <button type="button" className="ctl-feed-hit" aria-pressed={picked} onClick={() => select({ agent: item.agent, time: item.at })}>
        <time className="ctl-t" dateTime={new Date(item.at).toISOString()}>{clockAt(item.at)}</time>
        <span className="ctl-who">{who}</span>
        <span className="ctl-text" title={item.text}>
          {mark && <span className={`ctl-mark${mark.startsWith('▲') ? ' is-lit' : ''}`}>{mark}</span>}
          {afterName(item.text, who)}
        </span>
      </button>
    </li>
  );
}

function useRows(s: StatusSettings) {
  const { source, alarms } = useControl();
  const { selection } = useSelection();
  const raised = new Set(alarms.list.map((a) => a.id));
  const hidden = new Set(s.hidden ?? []);
  const markOf = (item: FeedItem) => {
    const id = alarmIdOf(item);
    if (!id || !raised.has(id)) return null;
    return alarms.acked.has(id) ? '✓ alarm, acknowledged' : '▲ alarm';
  };
  const rows = source.feed
    .filter((i) => !hidden.has(groupOf(i.kind)) || !!markOf(i))
    .filter((i) => !s.onlyAlarms || !!markOf(i))
    .filter((i) => !s.onlySelected || !selection.agent || i.agent === selection.agent)
    .slice(-SHOWN)
    .reverse();
  return { rows, markOf, selection };
}

export function StatusPane({ settings }: PaneProps) {
  const s = settings as StatusSettings;
  const { source } = useControl();
  const { rows, markOf, selection } = useRows(s);
  if (rows.length === 0) {
    const why = source.feed.length === 0 ? 'Nothing has happened yet.' : 'Nothing matches this pane’s filters. Change them in its Settings (⋯).';
    return <p className="ctl-empty">{why}</p>;
  }
  return (
    <ol className="ctl-feed" aria-label="Status feed, newest first">
      {rows.map((i) => <Row key={i.id} item={i} mark={markOf(i)} picked={selection.time === i.at && selection.agent === i.agent} />)}
    </ol>
  );
}

export function StatusSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as StatusSettings;
  const hidden = new Set(s.hidden ?? []);
  const toggle = (id: string, on: boolean) => setSettings({ ...s, hidden: KIND_GROUPS.map((g) => g.id).filter((g) => (g === id ? !on : hidden.has(g))) });
  return (
    <>
      <fieldset className="ctl-fieldset">
        <legend>Show</legend>
        {KIND_GROUPS.map((g) => (
          <label key={g.id} className="ctl-check"><input type="checkbox" checked={!hidden.has(g.id)} onChange={(e) => toggle(g.id, e.target.checked)} />{g.label}</label>
        ))}
      </fieldset>
      <label className="ctl-check"><input type="checkbox" checked={!!s.onlyAlarms} onChange={(e) => setSettings({ ...s, onlyAlarms: e.target.checked })} />Only alarms</label>
      <label className="ctl-check"><input type="checkbox" checked={!!s.onlySelected} onChange={(e) => setSettings({ ...s, onlySelected: e.target.checked })} />Only the selected agent</label>
    </>
  );
}
