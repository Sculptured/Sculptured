import type { AgentKind } from '../generated/AgentKind';
import type { DraftAgent } from '../generated/DraftAgent';
import { usesConnections } from '../draft/models';
import { connectionLabel, type ModelConnection } from '../hosted/connections';
import { KIND_LABEL } from '../vocab';

const MISSING = '__missing__';

/** A picker value: the kind, and the connection when it runs on one ("opencode@<id>"). */
const runsOnValue = (kind: AgentKind, connection?: string | null) => (connection ? `${kind}@${connection}` : kind);

/** What each connection runs: an Anthropic key, Claude Code; an OpenRouter key, OpenCode. A Claude agent
 * already on an OpenRouter key (Claude Code at OpenRouter's Anthropic-compatible address) keeps that choice. */
function connectionOptions(a: DraftAgent, conns: ModelConnection[]) {
  return conns.flatMap((c) => {
    const kind: AgentKind = c.provider === 'openrouter' ? 'opencode' : 'claude';
    const failed = c.checked?.ok === false ? ' (check failed)' : '';
    const legacy = c.provider === 'openrouter' && a.kind === 'claude' && a.connection === c.id;
    return [
      { value: runsOnValue(kind, c.id), label: `${connectionLabel(c)}${failed}` },
      ...(legacy ? [{ value: runsOnValue('claude', c.id), label: `Claude via OpenRouter · ${c.name}${failed}` }] : []),
    ];
  });
}

/** Hosted: Claude by subscription, each of your model connections, Codex, OpenCode (and Practice for a Practice agent). */
export function RunsOnPicker({ a, label, conns, disabled, onChange }: { a: DraftAgent; label: string; conns: ModelConnection[]; disabled: boolean; onChange: (kind: AgentKind, connection: string | null) => void }) {
  const known = usesConnections(a.kind) && a.connection ? conns.some((c) => c.id === a.connection) : true;
  const value = known ? runsOnValue(a.kind, usesConnections(a.kind) ? a.connection : null) : MISSING;
  return (
    <select aria-label={label} className="fld-input" value={value} disabled={disabled}
      onChange={(e) => {
        const [kind, connection] = e.target.value.split('@');
        if (e.target.value !== MISSING) onChange(kind as AgentKind, connection ?? null);
      }}>
      <option value="claude">Claude (subscription)</option>
      {connectionOptions(a, conns).map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
      {!known && <option value={MISSING} disabled>Removed connection</option>}
      <option value="codex">{KIND_LABEL.codex}</option>
      <option value="opencode">{KIND_LABEL.opencode} (its computer’s own setup)</option>
      {a.kind === 'practice' && <option value="practice">{KIND_LABEL.practice}</option>}
    </select>
  );
}
