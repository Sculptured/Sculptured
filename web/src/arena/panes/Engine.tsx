import { plural } from '../../format';
import type { BenchmarkInfo, Health } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate } from '../gate';
import { sizeWords } from '../words';

/** The agents Arena can run here, each with why not when it can't. */
function modes(h: Health) {
  return [
    { word: 'Claude Code', on: h.claude, why: 'claude is not on this computer' },
    { word: 'Codex', on: h.codex, why: 'codex is not on this computer' },
    { word: 'OpenRouter', on: h.open_available, why: 'needs OPENROUTER_API_KEY when arenad starts' },
  ];
}

function Benchmark({ b }: { b: BenchmarkInfo }) {
  return (
    <>
      <dt>Benchmark</dt>
      <dd><strong>{b.name}</strong>{b.version && <span className="mono muted"> {b.version}</span>}</dd>
      {b.error ? (
        <>
          <dt>Described</dt>
          <dd><span className="tst-status tst-blocked"><span aria-hidden="true">⚠</span>Can't be described</span> <span className="tst-evidence mono tst-small" data-raw-evidence>{b.error}</span></dd>
        </>
      ) : (
        <>
          <dt>Tasks</dt>
          <dd className="arena-num">{b.tasks != null ? plural(b.tasks, 'task') : 'Unknown'}</dd>
          <dt>World</dt>
          <dd>{b.world?.length ? b.world.map((p) => `${p.part} (${p.integration})`).join(', ') : 'Unknown'}</dd>
        </>
      )}
    </>
  );
}

function EngineBody() {
  const { health } = useArena();
  if (!health) return null;
  return (
    <dl className="arena-grid">
      <dt>Arena</dt>
      <dd><span className="arena-up"><span aria-hidden="true">●</span> Up</span> <span className="muted tst-small">answered its health check</span></dd>
      {modes(health).map((m) => (
        <div key={m.word} className="arena-grid-row">
          <dt>{m.word}</dt>
          <dd>{m.on ? <span><span aria-hidden="true">✓</span> Can run</span> : <span className="muted"><span aria-hidden="true">–</span> Can't run: {m.why}</span>}</dd>
        </div>
      ))}
      {health.disk && <><dt>Saves</dt><dd className="arena-num">{sizeWords(health.disk.bytes)} of {sizeWords(health.disk.limit_bytes)}</dd></>}
      {health.benchmark ? <Benchmark b={health.benchmark} /> : <><dt>Benchmark</dt><dd className="muted">Unknown: this Arena does not say</dd></>}
    </dl>
  );
}

/** Arena's health: whether it is up, which agents it can run, its disk use and the benchmark new runs use. */
export function EnginePane() {
  return <div className="arena-pane"><ArenaGate><EngineBody /></ArenaGate></div>;
}
