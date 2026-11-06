import { useState } from 'react';
import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { clockAt, nameIn, readingTime, useControl } from '../context';
import { actionRange, filterActions, lanesOf, type ActionFilter, type Placed } from '../plant/lanes';
import { EMPTY_PLANT, isAbnormal, kindCode, effectCode, type Plant, type PlantAction } from '../plant/model';
import { EffectMark, KindBadge, OutcomeMark } from '../plant/parts';
import { duration } from '../plant/trends';
import { ActionTrace } from './ActionTrace';

/**
 * Action timeline (a sequence-of-events recorder): a lane per agent, each action a mark at its time,
 * labelled by the system's kind and R or W. Writes are filled and heavier than reads; refusals and
 * errors are the only marks in colour, with their glyph. The list view has every action as a row.
 * Picking one shows its trace (display level 4): the request and the result as recorded.
 */

export type ActionsSettings = ActionFilter & { view?: 'lanes' | 'list'; window?: 'all' | '15m' | '5m' };
export const ACTIONS_DEFAULTS: ActionsSettings = { view: 'lanes', window: 'all' };
const WINDOW_MS = { all: undefined, '15m': 900_000, '5m': 300_000 } as const;

const markLabel = (a: PlantAction, plant: Plant) => `${kindCode(plant.systems.find((s) => s.id === a.system)?.kind ?? 'unknown')}·${effectCode(a.effect)}`;

function Mark({ m, plant, picked, pick }: { m: Placed; plant: Plant; picked: boolean; pick(a: PlantAction): void }) {
  const a = m.action;
  const cls = `pl-mark is-${a.effect}${isAbnormal(a.outcome) ? ` is-abnormal is-${a.outcome}` : ''}${picked ? ' is-picked' : ''}`;
  const sys = plant.systems.find((s) => s.id === a.system)?.name ?? a.system;
  return (
    <button type="button" className={cls} style={{ left: `${m.left}%`, top: `${m.row * 18 + 2}px` }} onClick={() => pick(a)}
      title={`${clockAt(a.at)} ${a.tool} on ${sys}: ${a.effect}, ${a.outcome}`} aria-label={`${clockAt(a.at)} ${a.tool} on ${sys}, ${a.effect}, ${a.outcome}`}>
      {isAbnormal(a.outcome) && <span aria-hidden="true">{a.outcome === 'refused' ? '⊘' : '✕'}</span>}{markLabel(a, plant)}
    </button>
  );
}

function Lanes({ actions, plant, picked, pick, window }: { actions: PlantAction[]; plant: Plant; picked?: string; pick(a: PlantAction): void; window?: number }) {
  const { source, now } = useControl();
  const range = actionRange(actions, readingTime(source, now), window);
  if (!range) return <p className="ctl-empty">No action matches. Actions appear here as agents use tools.</p>;
  const lanes = lanesOf(actions, source.agents.map((a) => a.id), range);
  return (
    <div className="pl-soe">
      <div className="pl-soe-axis" aria-hidden="true"><span /><span className="ctl-t">{clockAt(range.start)}</span><span className="ctl-t">{clockAt(range.end)}</span></div>
      <ul className="pl-soe-lanes" aria-label="Action lanes, one per agent">
        {lanes.map((l) => (
          <li key={l.agent} className="pl-soe-lane" style={{ minHeight: l.rows * 18 + 6 }}>
            <span className="pl-soe-who">{nameIn(source, l.agent)}</span>
            <span className="pl-soe-track">{l.marks.map((m) => <Mark key={m.action.id} m={m} plant={plant} picked={picked === m.action.id} pick={pick} />)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function List({ actions, plant, picked, pick }: { actions: PlantAction[]; plant: Plant; picked?: string; pick(a: PlantAction): void }) {
  const { source } = useControl();
  if (actions.length === 0) return <p className="ctl-empty">No action matches. Actions appear here as agents use tools.</p>;
  return (
    <table className="ctl-table pl-soe-list" aria-label="Actions, newest first">
      <thead><tr><th scope="col">Time</th><th scope="col">Agent</th><th scope="col">System</th><th scope="col">R/W</th><th scope="col">Tool</th><th scope="col" className="ctl-num">Took</th><th scope="col">Outcome</th></tr></thead>
      <tbody>
        {[...actions].reverse().slice(0, 500).map((a) => {
          const s = plant.systems.find((x) => x.id === a.system);
          return (
            <tr key={a.id} className={`${isAbnormal(a.outcome) ? 'is-abnormal' : ''}${picked === a.id ? ' is-picked' : ''}`} onClick={() => pick(a)}>
              <td className="ctl-t">{clockAt(a.at)}</td>
              <td>{nameIn(source, a.agent)}</td>
              <td>{s ? <><KindBadge s={s} /> {s.name}</> : a.system}</td>
              <td><EffectMark a={a} /></td>
              <td><button type="button" className="pl-linkish ctl-mono" onClick={() => pick(a)}>{a.tool}</button></td>
              <td className="ctl-num">{a.ms !== undefined ? duration(a.ms) : 'not recorded'}</td>
              <td><OutcomeMark o={a.outcome} /></td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function Filters({ s, setSettings, plant, shown, total }: { s: ActionsSettings; setSettings(v: ActionsSettings): void; plant: Plant; shown: number; total: number }) {
  return (
    <div className="pl-filters">
      <label className="ctl-field">System
        <select value={s.system ?? ''} onChange={(e) => setSettings({ ...s, system: e.target.value || undefined })}>
          <option value="">All systems</option>
          {plant.systems.map((x) => <option key={x.id} value={x.id}>{kindCode(x.kind)} · {x.name}</option>)}
        </select>
      </label>
      <label className="ctl-check"><input type="checkbox" checked={!!s.writesOnly} onChange={(e) => setSettings({ ...s, writesOnly: e.target.checked })} />Writes only</label>
      <label className="ctl-check"><input type="checkbox" checked={!!s.abnormalOnly} onChange={(e) => setSettings({ ...s, abnormalOnly: e.target.checked })} />Refused or failed only</label>
      <span className="pl-seg" role="group" aria-label="View">
        {(['lanes', 'list'] as const).map((v) => <button key={v} type="button" className={`btn btn-sm ${(s.view ?? 'lanes') === v ? 'btn-secondary' : 'btn-ghost'}`} aria-pressed={(s.view ?? 'lanes') === v} onClick={() => setSettings({ ...s, view: v })}>{v === 'lanes' ? 'Lanes' : 'List'}</button>)}
      </span>
      <span className="ctl-note">{shown} of {total} actions</span>
    </div>
  );
}

export function ActionsPane({ settings, setSettings }: PaneProps) {
  const s = settings as ActionsSettings;
  const { source } = useControl();
  const { select } = useSelection();
  const plant = source.plant ?? EMPTY_PLANT;
  const [picked, setPicked] = useState<string>();
  const actions = filterActions(plant.actions, s);
  const pick = (a: PlantAction) => { setPicked(a.id); select({ agent: a.agent, time: a.at }); };
  const chosen = plant.actions.find((a) => a.id === picked);
  const set = (v: ActionsSettings) => setSettings(v as Record<string, unknown>);
  return (
    <div className="pl-actions">
      <Filters s={s} setSettings={set} plant={plant} shown={actions.length} total={plant.actions.length} />
      {(s.view ?? 'lanes') === 'lanes'
        ? <Lanes actions={actions} plant={plant} picked={picked} pick={pick} window={WINDOW_MS[s.window ?? 'all']} />
        : <List actions={actions} plant={plant} picked={picked} pick={pick} />}
      {chosen ? <ActionTrace a={chosen} plant={plant} close={() => setPicked(undefined)} /> : plant.actions.length > 0 && <p className="ctl-note">Pick an action to see its request and result.</p>}
    </div>
  );
}

export function ActionsSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as ActionsSettings;
  return (
    <label className="ctl-field">Show
      <select value={s.window ?? 'all'} onChange={(e) => setSettings({ ...s, window: e.target.value as ActionsSettings['window'] })}>
        <option value="all">Whole run</option>
        <option value="15m">Last 15 minutes</option>
        <option value="5m">Last 5 minutes</option>
      </select>
    </label>
  );
}
