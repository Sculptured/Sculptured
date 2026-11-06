import { clockAt, nameIn, useControl } from '../context';
import { kindWords, type Plant, type PlantAction } from '../plant/model';
import { basisWords, EffectMark, KindBadge, OutcomeMark, PermitState, SystemName } from '../plant/parts';
import { duration } from '../plant/trends';

/**
 * One action's trace (display level 4): what it was, the system it touched and what that system is,
 * its effect and where that is known from, the outcome (the refusal or error in the system's own
 * words), the permit that gated it, then the request and the result exactly as recorded (evidence).
 */

const show = (v: unknown) => (v === undefined ? 'not recorded' : typeof v === 'string' ? v : JSON.stringify(v, null, 2));

export function ActionTrace({ a, plant, close }: { a: PlantAction; plant: Plant; close(): void }) {
  const { source } = useControl();
  const s = plant.systems.find((x) => x.id === a.system);
  const permit = a.permit ? plant.permits.find((p) => p.id === a.permit) : undefined;
  return (
    <section className="pl-trace" aria-label="Action trace">
      <header className="ctl-pane-head">
        <span><strong className="ctl-mono">{a.tool}</strong> · {nameIn(source, a.agent)} · <time className="ctl-t">{clockAt(a.at)}</time></span>
        <button type="button" className="btn btn-sm btn-ghost" onClick={close}>Close</button>
      </header>
      <dl className="pl-trace-facts">
        <dt>System</dt><dd>{s ? <><KindBadge s={s} /> <SystemName s={s} /> <span className="ctl-rule">({kindWords(s.kind)}; {basisWords(s.basis)})</span></> : a.system}</dd>
        <dt>Effect</dt><dd><EffectMark a={a} /> {a.effect === 'unknown' ? 'unknown' : a.effect} <span className="ctl-rule">({basisWords(a.effectBasis)})</span></dd>
        <dt>Outcome</dt><dd><OutcomeMark o={a.outcome} />{a.reason && <span className="pl-reason" data-raw-evidence>{a.reason}</span>}</dd>
        <dt>Took</dt><dd>{a.ms !== undefined ? duration(a.ms) : 'not recorded'}</dd>
        {a.to && <><dt>To</dt><dd>{a.to === '*' ? 'everyone' : a.to === 'next' ? 'whoever comes next' : nameIn(source, a.to)}</dd></>}
        {permit && <><dt>Permit</dt><dd><PermitState p={permit} /> · {permit.title}</dd></>}
      </dl>
      <div className="pl-trace-io">
        <figure><figcaption>Request</figcaption><pre data-raw-evidence>{show(a.request)}</pre></figure>
        <figure><figcaption>Result</figcaption><pre data-raw-evidence>{show(a.result)}</pre></figure>
      </div>
    </section>
  );
}
