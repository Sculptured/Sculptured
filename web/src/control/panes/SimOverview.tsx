import { compactNumber, plural } from '../../format';
import { CANT_RESTORE, restoreWords } from '../../arena/words';
import { planLine, savedParts, worldParts, type WorldPartLine } from '../../tests/restore';
import { KindMark } from '../../tests/shared';
import type { BranchDetail, Event, WorldPartInfo } from '../../tests/types';
import { useSelection } from '../../workspace/selection';
import { useControl } from '../context';
import type { AgentLine } from '../source';

/**
 * A simulated run's System overview: its agents (status, steps, what each reported at its end) and
 * a compact grid of the world's parts: how each restores, whether it keeps the arena clock, when
 * it was last saved on this branch, and how a fork restored it.
 */

type P = Record<string, unknown>;
const payload = (e: Event) => (e.payload ?? {}) as P;

function AgentRow({ a, b, picked, pick }: { a: AgentLine; b: BranchDetail; picked: boolean; pick(): void }) {
  const spec = b.team.agents.find((x) => x.id === a.id);
  const own = b.events.filter((e) => e.agent === a.id && e.kind !== 'agent_start' && e.kind !== 'agent_end');
  const end = b.events.find((e) => e.agent === a.id && e.kind === 'agent_end');
  const p = end ? payload(end) : {};
  return (
    <tr className={picked ? 'is-picked' : undefined}>
      <td><button type="button" className="ctl-key" aria-pressed={picked} onClick={pick}>{a.name}</button></td>
      <td>{spec ? <KindMark kind={spec.kind} model={spec.model} /> : <span className="ctl-muted">unknown</span>}</td>
      <td><span className={`ctl-glyph tone-${a.status.tone}`} aria-hidden="true">{a.status.glyph}</span> {a.status.word}{a.activity ? <span className="ctl-muted"> · {a.activity}</span> : null}</td>
      <td className="ctl-num">{own.length}</td>
      <td className="ctl-num">{typeof p.tokens === 'number' ? compactNumber(p.tokens) : 'unknown'}</td>
      <td className="ctl-num">{typeof p.seconds === 'number' ? `${p.seconds.toFixed(1)} s` : 'unknown'}</td>
    </tr>
  );
}

function Agents({ b }: { b: BranchDetail }) {
  const { source } = useControl();
  const { selection, select } = useSelection();
  return (
    <table className="ctl-table" aria-label="Agents">
      <thead><tr><th scope="col">Agent</th><th scope="col">Runs on</th><th scope="col">Status</th><th scope="col" className="ctl-num">Steps</th><th scope="col" className="ctl-num">Tokens</th><th scope="col" className="ctl-num">Time</th></tr></thead>
      <tbody>{source.agents.map((a) => <AgentRow key={a.id} a={a} b={b} picked={selection.agent === a.id} pick={() => select({ agent: a.id })} />)}</tbody>
    </table>
  );
}

/** The latest step after which `part` was saved, and how many tool calls ran on it. */
function partTrail(b: BranchDetail, part: string) {
  let saved: number | null = null;
  let calls = 0;
  for (const e of b.events) {
    if (e.kind !== 'tool_call') continue;
    const p = payload(e);
    if (p.part === part) calls += 1;
    if (savedParts(p).includes(part) && (saved === null || e.seq > saved)) saved = e.seq;
  }
  return { saved, calls };
}

function Restore({ info }: { info?: WorldPartInfo }) {
  if (!info?.answered) return <span className="ctl-muted">Restore unknown</span>;
  return (
    <span className="arena-chips">
      {restoreWords(info.restore).map((w) => <span key={w} className={`arena-chip${w === CANT_RESTORE ? ' arena-chip-none' : ''}`}>{w === CANT_RESTORE && <span aria-hidden="true">⊘ </span>}{w}</span>)}
    </span>
  );
}

function PartCard({ part, info, b }: { part: WorldPartLine; info?: WorldPartInfo; b: BranchDetail }) {
  const { saved, calls } = partTrail(b, part.part);
  const restored = b.restored?.find((r) => r.part === part.part);
  const clock = info?.uses_clock == null ? 'Clock unknown' : info.uses_clock ? '◷ Shakedown clock' : '○ Own time';
  return (
    <li className="ctl-part" aria-label={`Part ${part.part}`}>
      <div className="ctl-part-head"><b>{part.part}</b><span className="ctl-mono ctl-muted">{[part.integration, part.version].filter(Boolean).join(' · ')}</span></div>
      <Restore info={info} />
      <dl className="ctl-part-facts">
        <dt>Saved</dt><dd>{saved !== null ? <>◆ after step <span className="ctl-num">{saved}</span></> : 'not yet'}</dd>
        <dt>Tool calls</dt><dd className="ctl-num">{calls}</dd>
        <dt>Clock</dt><dd>{clock}</dd>
        {restored && <><dt>This fork</dt><dd>{planLine(restored)}</dd></>}
      </dl>
    </li>
  );
}

function Parts({ b }: { b: BranchDetail }) {
  const { source } = useControl();
  const parts = worldParts(b.events);
  if (parts.length === 0) return <p className="ctl-empty">This run was recorded before worlds had parts.</p>;
  const info = (name: string) => source.arena?.parts?.find((p) => p.name === name);
  return <ul className="ctl-parts" aria-label="World parts">{parts.map((p) => <PartCard key={p.part} part={p} info={info(p.part)} b={b} />)}</ul>;
}

export function SimOverview() {
  const { source } = useControl();
  const b = source.arena!.branch!;
  const parts = worldParts(b.events).length;
  return (
    <div className="ctl-simov">
      <h3 className="ctl-sub">Agents · {plural(source.agents.length, 'agent')}</h3>
      <Agents b={b} />
      <h3 className="ctl-sub">World · {plural(parts, 'part')}</h3>
      <Parts b={b} />
    </div>
  );
}
