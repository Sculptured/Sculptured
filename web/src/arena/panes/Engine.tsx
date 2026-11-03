import { plural } from '../../format';
import { Btn } from '../../shell/ui';
import { providerState, stateGlyph, stateReason, useProviderList } from '../../tests/providers';
import { ErrorNote } from '../../tests/shared';
import type { BenchmarkInfo, Health, ProviderStatus } from '../../tests/types';
import { useArena } from '../data';
import { ArenaGate } from '../gate';
import { sizeWords } from '../words';

/** The CLI agents Arena can run here, each with why not when it can't; API mode's providers have their own rows. */
function modes(h: Health) {
  const cli = [
    { word: 'Claude Code', on: h.claude, why: 'claude is not on this computer' },
    { word: 'Codex', on: h.codex, why: 'codex is not on this computer' },
  ];
  // An Arena from before providers says only whether API mode can run.
  return h.providers ? cli : [...cli, { word: 'API mode', on: h.open_available, why: 'no model provider is set up when arenad starts' }];
}

const STATE_WORD = { ready: 'Ready', unavailable: 'Unavailable', 'not checked': 'Not checked' } as const;

/** One row per model provider: glyph, word and plain reason; Check asks each provider (`GET /api/providers`). */
function Providers({ health }: { health: ProviderStatus[] }) {
  const { list, loading, error, check } = useProviderList(health);
  const unchecked = list.some((s) => providerState(s) === 'not checked');
  return (
    <>
      {list.map((s) => {
        const st = providerState(s);
        const why = stateReason(s);
        return (
          <div key={s.id} className="arena-grid-row">
            <dt>{s.label}</dt>
            <dd className={st === 'ready' ? undefined : 'muted'}><span aria-hidden="true">{stateGlyph(st)}</span> {STATE_WORD[st]}{why && <span className="tst-small">: {why}</span>}</dd>
          </div>
        );
      })}
      <dt>Providers</dt>
      <dd>
        <Btn size="sm" variant="ghost" onClick={check} disabled={loading}>{loading ? 'Checking…' : unchecked ? 'Check' : 'Check again'}</Btn>
        <span className="muted tst-small"> asks each local server, and OpenAI and Anthropic when keyed, for its models</span>
        <ErrorNote error={error} />
      </dd>
    </>
  );
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
      {health.providers && <Providers health={health.providers} />}
      {health.disk && <><dt>Saves</dt><dd className="arena-num">{sizeWords(health.disk.bytes)} of {sizeWords(health.disk.limit_bytes)}</dd></>}
      {health.benchmark ? <Benchmark b={health.benchmark} /> : <><dt>Benchmark</dt><dd className="muted">Unknown: this Arena does not say</dd></>}
    </dl>
  );
}

/** Arena's health: whether it is up, which agents and model providers it can run, its disk use and the benchmark new runs use. */
export function EnginePane() {
  return <div className="arena-pane"><ArenaGate><EngineBody /></ArenaGate></div>;
}
