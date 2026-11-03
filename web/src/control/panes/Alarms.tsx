import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { RULE_WORDS, type Alarm, type AlarmRule } from '../alarms';
import { afterName, clockAt, nameIn, useControl } from '../context';

/**
 * Alarms: the master caution list, newest first. Each is acknowledged on its own (kept per team);
 * an acknowledged alarm stays listed, marked so. The rules and their thresholds are in Settings.
 */

function Row({ a }: { a: Alarm }) {
  const { source, alarms } = useControl();
  const { select } = useSelection();
  const acked = alarms.acked.has(a.id);
  const who = nameIn(source, a.agent);
  return (
    <li className={`ctl-alarm${acked ? ' is-acked' : ' is-lit'}`}>
      <span className="ctl-alarm-state">{acked ? '✓ acknowledged' : '▲ new'}</span>
      <button type="button" className="ctl-alarm-what" onClick={() => select({ time: a.at, ...(a.agent ? { agent: a.agent } : {}) })}>
        <time className="ctl-t">{clockAt(a.at)}</time>
        <span className="ctl-who">{who}</span>
        <span className="ctl-alarm-text">{afterName(a.text, who)} <span className="ctl-rule">· {RULE_WORDS[a.rule] ?? a.rule}</span></span>
      </button>
      {!acked && <button type="button" className="btn btn-secondary btn-sm" onClick={() => alarms.ack(a.id)}>Acknowledge</button>}
    </li>
  );
}

export function AlarmsPane() {
  const { alarms } = useControl();
  const on = alarms.rules.filter((r) => r.on).length;
  if (alarms.list.length === 0)
    return <p className="ctl-empty">○ No alarms. {on} of {alarms.rules.length} rules are on (Settings, ⋯).</p>;
  return (
    <div className="ctl-alarms">
      <p className="ctl-pane-head">
        <span><strong className={alarms.unacked > 0 ? 'ctl-lit' : undefined}>{alarms.unacked}</strong> not acknowledged · {alarms.list.length} in all</span>
        {alarms.unacked > 1 && <button type="button" className="btn btn-sm btn-ghost" onClick={alarms.ackAll}>Acknowledge all</button>}
      </p>
      <ol className="ctl-alarm-list" aria-label="Alarms, newest first">
        {[...alarms.list].reverse().map((a) => <Row key={a.id} a={a} />)}
      </ol>
    </div>
  );
}

function RuleRow({ rule, set }: { rule: AlarmRule; set(r: AlarmRule): void }) {
  return (
    <li className="ctl-rule-row">
      <label className="ctl-check"><input type="checkbox" checked={rule.on} onChange={(e) => set({ ...rule, on: e.target.checked })} />{RULE_WORDS[rule.id] ?? rule.id}</label>
      {rule.threshold !== undefined && (
        <label className="ctl-field ctl-threshold">after
          <input type="number" min={0} step={1} value={Math.round(rule.threshold / 60)} disabled={!rule.on}
            onChange={(e) => { const m = Number(e.target.value); if (Number.isFinite(m) && m >= 0) set({ ...rule, threshold: m * 60 }); }} />
          min
        </label>
      )}
    </li>
  );
}

/** The rules are the team's, not this pane's: every Alarms pane and the header's count use them. */
export function AlarmsSettingsForm(_: PaneProps) {
  const { alarms } = useControl();
  const set = (r: AlarmRule) => alarms.setRules(alarms.rules.map((x) => (x.id === r.id ? r : x)));
  return (
    <fieldset className="ctl-fieldset">
      <legend>Raise an alarm when</legend>
      <ul className="ctl-rules">{alarms.rules.map((r) => <RuleRow key={r.id} rule={r} set={set} />)}</ul>
    </fieldset>
  );
}
