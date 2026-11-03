import type { KeyboardEvent, MouseEvent } from 'react';
import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { alarmIdOf, clockAt, useControl } from '../context';
import type { AgentLine } from '../source';
import { alarmMarks, axisOf, inRange, marksOf, pct, rangeOf, stepTime, turnsOf, WINDOWS, type Bar, type Mark, type Range, type Window } from './timelineModel';

/**
 * The Timeline: one thin lane per agent (its turns as bars, everything else as marks) and an alarms
 * lane, with the selection's time as the cursor. Clicking a lane selects that moment and agent; on
 * the keyboard each lane is a slider (arrows, Shift for bigger steps, Home and End).
 */

export type TimelineSettings = { window?: Window; hiddenLanes?: string[]; alarmsLane?: boolean };
export const TIMELINE_DEFAULTS: TimelineSettings = { window: 'all', hiddenLanes: [], alarmsLane: true };

type LaneProps = { label: React.ReactNode; name: string; agent?: string; range: Range; bars: Bar[]; marks: Mark[]; cursor?: number; picked: boolean };

function Track({ name, agent, range, bars, marks, cursor }: Omit<LaneProps, 'label' | 'picked'>) {
  const { select } = useSelection();
  const at = cursor !== undefined && inRange(range, cursor) ? cursor : range.end;
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    const f = box.width > 0 ? (e.clientX - box.left) / box.width : 1;
    select({ time: Math.round(range.start + Math.min(1, Math.max(0, f)) * (range.end - range.start)), ...(agent ? { agent } : {}) });
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const next = stepTime(range, at, e.key, e.shiftKey);
    if (next === null) return;
    e.preventDefault();
    select({ time: Math.round(next), ...(agent ? { agent } : {}) });
  };
  return (
    <div className="ctl-track" role="slider" tabIndex={0} aria-label={`${name}: time`} aria-valuemin={range.start} aria-valuemax={range.end} aria-valuenow={at}
      aria-valuetext={clockAt(at)} onClick={onClick} onKeyDown={onKey}>
      {bars.map((b) => <span key={`b${b.from}`} className="ctl-bar" style={{ left: `${pct(range, b.from)}%`, width: `${Math.max(0.4, pct(range, b.to) - pct(range, b.from))}%` }} />)}
      {marks.map((m) => <span key={m.id} className={`ctl-tick is-${m.tone}`} style={{ left: `${pct(range, m.at)}%` }} title={`${clockAt(m.at)} ${m.text}`} />)}
      {cursor !== undefined && inRange(range, cursor) && <span className="ctl-cursor" style={{ left: `${pct(range, cursor)}%` }} aria-hidden="true" />}
    </div>
  );
}

function Lane({ label, picked, ...track }: LaneProps) {
  return (
    <li className={`ctl-lane${picked ? ' is-picked' : ''}`} data-lane={track.agent ?? 'alarms'}>
      <span className="ctl-lane-name">{label}</span>
      <Track {...track} />
    </li>
  );
}

function AgentLabel({ a, picked }: { a: AgentLine; picked: boolean }) {
  return (
    <>
      <span className={`ctl-glyph tone-${a.status.tone}`} aria-hidden="true">{a.status.glyph}</span>
      <span className="ctl-lane-who">{picked && <span className="sr-only">Selected: </span>}{a.name}</span>
      <span className="ctl-lane-word">{a.status.word}</span>
    </>
  );
}

function Axis({ range, cursor }: { range: Range; cursor?: number }) {
  return (
    <div className="ctl-axis" aria-hidden="true">
      <span className="ctl-lane-name">{cursor !== undefined ? <>▼ {clockAt(cursor)}</> : <span className="ctl-cursor-none">Cursor —</span>}</span>
      <span className="ctl-axis-ticks">{axisOf(range).map((t) => <span key={t} className="ctl-t">{clockAt(t)}</span>)}</span>
    </div>
  );
}

export function TimelinePane({ settings }: PaneProps) {
  const s = settings as TimelineSettings;
  const { source, alarms, now } = useControl();
  const { selection } = useSelection();
  const range = rangeOf(source.feed, s.window ?? 'all', now);
  if (!range) return <p className="ctl-empty">The timeline fills in once the team does something.</p>;
  const hidden = new Set(s.hiddenLanes ?? []);
  const ids = new Set(alarms.list.map((a) => a.id));
  const lanes = source.agents.filter((a) => !hidden.has(a.id)).map((a) => {
    const items = source.feed.filter((i) => i.agent === a.id);
    return { a, bars: turnsOf(items, range.end), marks: marksOf(items, ids, alarmIdOf) };
  });
  return (
    <div className="ctl-timeline">
      <Axis range={range} cursor={selection.time} />
      <ul className="ctl-lanes" aria-label="Timeline lanes">
        {lanes.map(({ a, bars, marks }) => (
          <Lane key={a.id} name={a.name} agent={a.id} label={<AgentLabel a={a} picked={selection.agent === a.id} />} range={range} bars={bars} marks={marks} cursor={selection.time} picked={selection.agent === a.id} />
        ))}
        {s.alarmsLane !== false && (
          <Lane name="Alarms" label={<><span className="ctl-glyph tone-bad" aria-hidden="true">▲</span><span className="ctl-lane-who">Alarms</span><span className="ctl-lane-word">{alarms.list.length}</span></>}
            range={range} bars={[]} marks={alarmMarks(alarms.list)} cursor={selection.time} picked={false} />
        )}
      </ul>
    </div>
  );
}

export function TimelineSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as TimelineSettings;
  const { source } = useControl();
  const hidden = new Set(s.hiddenLanes ?? []);
  const toggle = (id: string, on: boolean) => setSettings({ ...s, hiddenLanes: on ? [...hidden].filter((x) => x !== id) : [...hidden, id] });
  return (
    <>
      <label className="ctl-field">Show
        <select value={s.window ?? 'all'} onChange={(e) => setSettings({ ...s, window: e.target.value as Window })}>
          {WINDOWS.map((w) => <option key={w.id} value={w.id}>{w.label}</option>)}
        </select>
      </label>
      <fieldset className="ctl-fieldset">
        <legend>Lanes</legend>
        {source.agents.map((a) => <label key={a.id} className="ctl-check"><input type="checkbox" checked={!hidden.has(a.id)} onChange={(e) => toggle(a.id, e.target.checked)} />{a.name}</label>)}
        <label className="ctl-check"><input type="checkbox" checked={s.alarmsLane !== false} onChange={(e) => setSettings({ ...s, alarmsLane: e.target.checked })} />Alarms</label>
      </fieldset>
    </>
  );
}
