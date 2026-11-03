import { plural } from '../../format';
import { EngineLine } from '../../tests/shared';
import type { BenchmarkInfo } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate } from '../gate';

function Benchmark({ b }: { b: BenchmarkInfo }) {
  return (
    <section className="arena-bench" aria-label="Benchmark">
      <p><span className="tst-label">Benchmark</span> <strong>{b.name}</strong>{b.version && <span className="muted mono"> {b.version}</span>}</p>
      {b.error ? (
        <p className="tst-small"><span className="tst-status tst-blocked"><span aria-hidden="true">⚠</span>Can't be described</span>{' '}
          <span className="tst-evidence mono" data-raw-evidence>{b.error}</span>
        </p>
      ) : (
        <p className="tst-small">
          {b.tasks != null ? plural(b.tasks, 'task') : 'Tasks unknown'}
          {b.world?.length ? ` · world: ${b.world.map((p) => `${p.part} (${p.integration})`).join(', ')}` : ''}
        </p>
      )}
    </section>
  );
}

function EngineBody() {
  const { health } = useArena();
  if (!health) return null;
  return (
    <>
      <EngineLine health={health} />
      {health.benchmark ? <Benchmark b={health.benchmark} /> : <p className="muted tst-small">Benchmark unknown: this Arena does not say.</p>}
    </>
  );
}

/** Arena's health: which agents it can run, its disk use and the benchmark new runs use. */
export function EnginePane() {
  return <div className="arena-pane"><ArenaGate><EngineBody /></ArenaGate></div>;
}
