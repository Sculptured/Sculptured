import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '../api';
import { errorText } from '../errors';
import { VendorMark } from '../shell/ui';
import { arena } from './api';
import { kindLabel } from './team';
import type { AgentKind, BranchStatus, Health } from './types';

const STATUS: Record<BranchStatus, { glyph: string; word: string; tone: string }> = {
  running: { glyph: '◐', word: 'Running', tone: 'working' },
  passed: { glyph: '✓', word: 'Passed the check', tone: 'done' },
  failed: { glyph: '✕', word: 'Failed the check', tone: 'failed' },
  infra_error: { glyph: '⚠', word: "Couldn't run", tone: 'blocked' },
  interrupted: { glyph: '■', word: 'Stopped', tone: 'unknown' },
};

/** A branch's status: its shape and its word, then colour. */
export function StatusPill({ status, short }: { status: BranchStatus; short?: boolean }) {
  const s = STATUS[status];
  return (
    <span className={`tst-status tst-${s.tone}`}>
      <span aria-hidden="true">{s.glyph}</span>
      {short ? s.word.replace(' the check', '') : s.word}
    </span>
  );
}

/** The provider tile and name for an agent's mode. */
export function KindMark({ kind, model }: { kind: AgentKind; model?: string | null }) {
  const v = kind === 'claude_code' ? { mark: '✳', name: 'Claude', cls: 'v-claude' }
    : kind === 'codex' ? { mark: '◈', name: 'Codex', cls: 'v-codex' }
    : { mark: '▪', name: 'OpenRouter', cls: 'v-other' };
  return (
    <span className="tst-kind">
      <VendorMark v={v} size="xs" />
      <span>{kindLabel(kind)}</span>
      {model && <span className="mono muted">{model}</span>}
    </span>
  );
}

/** A plain sentence for any error; the engine's own words follow as evidence when it gave some. */
export function ErrorNote({ error }: { error: unknown }) {
  if (error == null) return null;
  const raw = error instanceof ApiError ? error.raw : undefined;
  return (
    <div className="tst-note tst-note-error" role="alert">
      <span>{errorText(error)}</span>
      {raw && <span className="tst-evidence mono" data-raw-evidence>Test engine: {raw}</span>}
    </div>
  );
}

/** Tells the rest of the Tests pages to re-read the engine's status (disk use changes after a delete). */
export const HEALTH_CHANGED = 'sculptured:tests-health';

/** The engine's status: undefined while asking, null when it is not answering. */
export function useArenaHealth(): { health: Health | null | undefined; error: unknown } {
  const [health, setHealth] = useState<Health | null | undefined>(undefined);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    const load = () =>
      arena.health().then((h) => { if (alive) { setHealth(h); setError(null); } })
        .catch((e) => { if (alive) { setHealth(null); setError(e); } });
    load();
    const t = setInterval(load, 15000);
    window.addEventListener(HEALTH_CHANGED, load);
    return () => { alive = false; clearInterval(t); window.removeEventListener(HEALTH_CHANGED, load); };
  }, []);
  return { health, error };
}

const gb = (n: number) => { const v = n / 2 ** 30; return v >= 10 ? Math.round(v).toString() : (Math.round(v * 10) / 10).toString(); };

/** Which agent modes the engine can run, and how much of its disk budget the test databases use. */
export function EngineLine({ health }: { health: Health }) {
  const modes = [
    { on: health.claude, word: 'Claude Code', why: 'claude is not on this computer' },
    { on: health.codex, word: 'Codex', why: 'codex is not on this computer' },
    { on: health.open_available, word: 'OpenRouter', why: 'needs OPENROUTER_API_KEY when arenad starts' },
  ];
  return (
    <span className="tst-engine">
      <span className="tst-engine-dot" aria-hidden="true">●</span>
      <span>Test engine running</span>
      {modes.map((m) => (
        <span key={m.word} className={m.on ? '' : 'muted'}>{m.on ? '✓' : '–'} {m.word}{m.on ? '' : ` (${m.why})`}</span>
      ))}
      {health.disk && <span className="muted">Databases {gb(health.disk.bytes)} of {gb(health.disk.limit_bytes)} GB</span>}
    </span>
  );
}

/** One headline number with its label and, under it, where it came from. */
export function Stat({ label, value, note }: { label: string; value: ReactNode; note?: string }) {
  return (
    <div className="tst-stat">
      <span className="tst-stat-label">{label}</span>
      <span className="tst-stat-value">{value}</span>
      {note && <span className="tst-stat-note">{note}</span>}
    </div>
  );
}

export function formatWhen(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
}

export const pretty = (v: unknown) => (typeof v === 'string' ? v : JSON.stringify(v ?? null, null, 2));
