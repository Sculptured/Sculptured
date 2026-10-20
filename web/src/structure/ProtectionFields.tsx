import type { Protection } from '../generated/Protection';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Edit } from './Panels';
import { SafetyFields } from './SafetyFields';

export const DEFAULT_PROTECTION: Protection = { on: true, read_kb: 100, shell_chars: 8000, context_tokens: 60000 };

const same = (a: Protection, b: Protection) => a.on === b.on && a.read_kb === b.read_kb && a.shell_chars === b.shell_chars && a.context_tokens === b.context_tokens;

/** The draft with its protections changed; the default is not written. */
export function withProtection(d: TeamDraft, patch: Partial<Protection>): TeamDraft {
  const next = { ...DEFAULT_PROTECTION, ...(d.protection ?? {}), ...patch };
  const { protection: _old, ...rest } = d;
  return same(next, DEFAULT_PROTECTION) ? rest : { ...rest, protection: next };
}

type NumberKey = 'read_kb' | 'shell_chars' | 'context_tokens';
const NUMBERS: { key: NumberKey; label: string; unit: string; min: number }[] = [
  { key: 'read_kb', label: 'Refuse reading a whole file larger than', unit: 'KB', min: 1 },
  { key: 'shell_chars', label: 'Shell output an agent sees, at most', unit: 'characters', min: 500 },
  { key: 'context_tokens', label: 'Continue an agent fresh from its notes at', unit: 'tokens of context', min: 30000 },
];

/** Rules between the team's agents and their tools, and which agents each one reaches. */
export function ProtectionFields({ draft, edit, locked }: { draft: TeamDraft; edit: Edit; locked: boolean }) {
  const p = { ...DEFAULT_PROTECTION, ...(draft.protection ?? {}) };
  const set = (patch: Partial<Protection>) => edit((d) => withProtection(d, patch), 'protection');
  return (
    <fieldset className="ta-limits">
      <legend className="fld-label">Protections</legend>
      <label className="ta-check">
        <input id="protection-on" type="checkbox" checked={p.on} disabled={locked} onChange={(e) => set({ on: e.target.checked })} />
        <span><b>Protections</b><span className="fld-meta">Keep agents’ context small: refuse whole reads of large files, cap shell output, and continue an agent fresh from its notes when its conversation grows large. Every refusal shows on the agent’s page.</span></span>
      </label>
      {NUMBERS.map((n) => (
        <label key={n.key} className="ta-number">
          <span>{n.label}</span>
          <input type="number" min={n.min} value={p[n.key]} disabled={locked || !p.on} aria-label={`${n.label} (${n.unit})`}
            onChange={(e) => { const v = Math.round(Number(e.target.value)); if (Number.isFinite(v) && v >= n.min) set({ [n.key]: v } as Partial<Protection>); }} />
          <span className="fld-meta">{n.unit}</span>
        </label>
      ))}
      <p className="fld-meta">Claude agents: all three. Codex agents: the context limit only (and Codex compacts its conversation within a long turn at twice it). An agent on a computer whose runner predates protections runs without them, and its page says so. Takes effect the next time an agent starts.</p>
      {p.on && <SafetyFields draft={draft} edit={edit} locked={locked} />}
    </fieldset>
  );
}
