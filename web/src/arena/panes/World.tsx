import { useEffect, useState } from 'react';
import { plural } from '../../format';
import { arena } from '../../tests/api';
import type { WorldPartInfo, WorldResponse } from '../../tests/types';
import { ArenaGate, ArenaProblem } from '../gate';
import { CANT_RESTORE, clockWords, restoreWords } from '../words';

/** The world's parts, asked once Arena answers. */
function useWorld() {
  const [world, setWorld] = useState<WorldResponse | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    arena.world().then((w) => { if (alive) { setWorld(w); setError(null); } }).catch((e) => { if (alive) setError(e); });
    return () => { alive = false; };
  }, []);
  return { world, error };
}

function Answered({ part }: { part: WorldPartInfo }) {
  return part.answered
    ? <span className="tst-status tst-done"><span aria-hidden="true">✓</span>Answered</span>
    : <span className="tst-status tst-blocked"><span aria-hidden="true">⚠</span>Not answering</span>;
}

function Tools({ tools }: { tools: string[] }) {
  if (!tools.length) return <span className="muted">No tools</span>;
  return (
    <details className="arena-tools">
      <summary>{plural(tools.length, 'tool')}</summary>
      <ul>{tools.map((t) => <li key={t} className="mono">{t}</li>)}</ul>
    </details>
  );
}

function Facts({ part }: { part: WorldPartInfo }) {
  return (
    <dl className="arena-facts">
      <dt>Restore</dt>
      <dd>{restoreWords(part.restore).map((w) => <span key={w} className={`arena-chip${w === CANT_RESTORE ? ' arena-chip-none' : ''}`}>{w}</span>)}</dd>
      <dt>Clock</dt>
      <dd>{clockWords(part.uses_clock)}</dd>
      <dt>Tools</dt>
      <dd>{part.tools ? <Tools tools={part.tools} /> : <span className="muted">Unknown</span>}</dd>
    </dl>
  );
}

function Part({ part }: { part: WorldPartInfo }) {
  return (
    <li className="arena-item" aria-label={`Part ${part.name}`}>
      <div className="arena-item-head">
        <strong>{part.name}</strong>
        <span className="muted mono tst-small">{part.integration}{part.version ? ` ${part.version}` : ''}</span>
        <span className="tst-spacer" />
        <Answered part={part} />
      </div>
      {part.answered ? <Facts part={part} /> : (
        <p className="tst-small">Its integration did not describe itself, so how it restores is unknown. It said:{' '}
          <span className="tst-evidence mono" data-raw-evidence>{part.error ?? 'nothing'}</span>
        </p>
      )}
    </li>
  );
}

function WorldBody() {
  const { world, error } = useWorld();
  if (error) return <ArenaProblem error={error} />;
  if (!world) return <p className="muted">Asking each part…</p>;
  if (!world.parts.length) return <p className="muted">This benchmark's world has no parts.</p>;
  return <ul className="arena-list">{world.parts.map((p) => <Part key={p.name} part={p} />)}</ul>;
}

/** The benchmark's parts: integration and version, how each restores, its clock and its tools. */
export function WorldPane() {
  return <div className="arena-pane"><ArenaGate><WorldBody /></ArenaGate></div>;
}
