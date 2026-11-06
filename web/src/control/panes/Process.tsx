import { useState } from 'react';
import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { nameIn, readingTime, useControl } from '../context';
import { agentLinks, flowsOf, ratePerMin, type Flow } from '../plant/flows';
import { EMPTY_PLANT, type Permit, type Plant, type SystemUnit } from '../plant/model';
import { KindBadge, PermitState, SystemName } from '../plant/parts';
import type { AgentLine } from '../source';

/**
 * Process view (display level 2): the team's agents as units on the left, the systems they act on as
 * equipment on the right, and a line for each flow of actions between them: heavier for writes,
 * dotted where the effect is unknown, marked abnormal where a call was refused or failed, and moving
 * while it is live. Interlocks sit on the units (each agent's rights) and on the approvals equipment
 * (its permits). The same flows are listed as text under the picture. Layout is fixed by order, so
 * live updates never move a unit.
 */

const ROW = 64;

function y(i: number) { return i * ROW + ROW / 2; }

function flowClass(f: Flow, focus: string | null) {
  const kind = f.writes > 0 ? 'is-write' : f.reads > 0 ? 'is-read' : 'is-unknown';
  const dim = focus && f.system !== focus && f.agent !== focus ? ' is-dim' : '';
  return `pl-flow ${kind}${f.abnormal ? ' is-abnormal' : ''}${f.live ? ' is-live' : ''}${dim}`;
}

function Lines({ agents, systems, flows, focus, height }: { agents: string[]; systems: SystemUnit[]; flows: Flow[]; focus: string | null; height: number }) {
  return (
    <svg className="pl-lines" viewBox={`0 0 100 ${height}`} preserveAspectRatio="none" style={{ height }} aria-hidden="true">
      {flows.map((f) => {
        const a = agents.indexOf(f.agent);
        const s = systems.findIndex((x) => x.id === f.system);
        if (a < 0 || s < 0) return null;
        return <line key={`${f.agent}:${f.system}`} className={flowClass(f, focus)} x1={0} y1={y(a)} x2={100} y2={y(s)} vectorEffect="non-scaling-stroke" />;
      })}
    </svg>
  );
}

function Unit({ a, plant, now, focus, onFocus }: { a: AgentLine; plant: Plant; now: number; focus: string | null; onFocus(id: string): void }) {
  const { select } = useSelection();
  const mine = plant.actions.filter((x) => x.agent === a.id);
  const rights = plant.rights[a.id] ?? [];
  const abnormal = mine.filter((x) => x.outcome === 'refused' || x.outcome === 'error').length;
  return (
    <li className={`pl-unit${focus === a.id ? ' is-focus' : ''}${abnormal ? ' is-abnormal' : ''}`} style={{ height: ROW }}>
      <button type="button" className="pl-unit-hit" onClick={() => { onFocus(a.id); select({ agent: a.id }); }} aria-pressed={focus === a.id}>
        <span className="pl-unit-top"><span className={`ctl-glyph tone-${a.status.tone}`} aria-hidden="true">{a.status.glyph}</span><strong>{a.name}</strong><span className="pl-unit-word">{a.status.word}</span></span>
        <span className="pl-unit-fig">{ratePerMin(mine, now).toFixed(1)}/min · {mine.length} actions{abnormal ? <span className="pl-abn"> · ⊘ {abnormal} refused or failed</span> : null}</span>
        {rights.length > 0 && <span className="pl-rights" title={rights.join('\n')}>⚿ {rights[0]}{rights.length > 1 ? ` (+${rights.length - 1})` : ''}</span>}
      </button>
    </li>
  );
}

function Equipment({ s, plant, focus, onFocus }: { s: SystemUnit; plant: Plant; focus: string | null; onFocus(id: string): void }) {
  const mine = plant.actions.filter((x) => x.system === s.id);
  const reads = mine.filter((x) => x.effect === 'read').length;
  const writes = mine.filter((x) => x.effect === 'write').length;
  const abnormal = mine.filter((x) => x.outcome === 'refused' || x.outcome === 'error').length;
  const waiting = s.kind === 'approvals' ? plant.permits.filter((p) => p.state === 'waiting').length : 0;
  return (
    <li className={`pl-equip${focus === s.id ? ' is-focus' : ''}${abnormal ? ' is-abnormal' : ''}`} style={{ height: ROW }}>
      <button type="button" className="pl-unit-hit" onClick={() => onFocus(s.id)} aria-pressed={focus === s.id}>
        <span className="pl-unit-top"><KindBadge s={s} /><SystemName s={s} /></span>
        <span className="pl-unit-fig">R {reads} · W {writes}{mine.length - reads - writes ? ` · ? ${mine.length - reads - writes}` : ''}{abnormal ? <span className="pl-abn"> · ⊘ {abnormal}</span> : null}</span>
        {s.kind === 'approvals' && <span className="pl-rights">⚿ interlock · {waiting} permit{waiting === 1 ? '' : 's'} waiting</span>}
      </button>
    </li>
  );
}

function FlowList({ flows, plant, links }: { flows: Flow[]; plant: Plant; links: ReturnType<typeof agentLinks> }) {
  const { source } = useControl();
  const sys = (id: string) => plant.systems.find((s) => s.id === id)?.name ?? id;
  if (flows.length === 0) return null;
  return (
    <details className="pl-flowlist">
      <summary>Flows as a list ({flows.length})</summary>
      <ul>
        {flows.map((f) => (
          <li key={`${f.agent}:${f.system}`}>{nameIn(source, f.agent)} → {sys(f.system)}: {f.reads} read · {f.writes} write · {f.other} effect unknown
            {f.abnormal ? <strong className="pl-abn"> · ⊘ {f.abnormal} refused or failed</strong> : null}{f.live ? ' · ● live' : ''}</li>
        ))}
        {links.map((l) => <li key={`${l.from}>${l.to}`}>{nameIn(source, l.from)} → {nameIn(source, l.to)}: {l.count} message{l.count === 1 ? '' : 's'}{l.live ? ' · ● live' : ''}</li>)}
      </ul>
    </details>
  );
}

function Permits({ permits }: { permits: Permit[] }) {
  const { source } = useControl();
  if (permits.length === 0) return null;
  return (
    <section className="pl-permits" aria-label="Interlocks: permits">
      <h3 className="ctl-sub">Interlocks · permits</h3>
      <ul>{[...permits].reverse().slice(0, 12).map((p) => <li key={p.id}><PermitState p={p} /> <span className="pl-permit-what">{p.title}</span> <span className="ctl-rule">· {nameIn(source, p.agent)}{p.system ? ` · ${p.system}` : ''}</span></li>)}</ul>
    </section>
  );
}

export function ProcessPane(_: PaneProps) {
  const { source, now: clock } = useControl();
  const now = readingTime(source, clock);
  const plant = source.plant ?? EMPTY_PLANT;
  const [focus, setFocus] = useState<string | null>(null);
  const toggle = (id: string) => setFocus((f) => (f === id ? null : id));
  if (source.agents.length === 0 && plant.systems.length === 0)
    return <p className="ctl-empty">The process view shows the team's agents and the systems they act on once the team has agents.</p>;
  const ids = source.agents.map((a) => a.id);
  const flows = flowsOf(plant.actions, now);
  const height = Math.max(ids.length, plant.systems.length, 1) * ROW;
  return (
    <div className="pl-process">
      <div className="pl-grid" style={{ height }}>
        <ol className="pl-col" aria-label="Agents (units)">{source.agents.map((a) => <Unit key={a.id} a={a} plant={plant} now={now} focus={focus} onFocus={toggle} />)}</ol>
        <Lines agents={ids} systems={plant.systems} flows={flows} focus={focus} height={height} />
        <ol className="pl-col" aria-label="Systems (equipment)">
          {plant.systems.length ? plant.systems.map((s) => <Equipment key={s.id} s={s} plant={plant} focus={focus} onFocus={toggle} />)
            : <li className="ctl-empty">No system yet: equipment appears as agents use tools.</li>}
        </ol>
      </div>
      <p className="ctl-note">Lines: heavy for writes, thin for reads, dotted where the effect is unknown; ⊘ marks refused or failed calls; ● live in the last minute.</p>
      <FlowList flows={flows} plant={plant} links={agentLinks(plant.actions, ids, now)} />
      <Permits permits={plant.permits} />
    </div>
  );
}
