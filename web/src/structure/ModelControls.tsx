import './agent-details.css';
import { useId, useState } from 'react';
import type { AgentKind } from '../generated/AgentKind';
import type { ModelKind } from '../draft/models';
import { VendorMark } from '../shell/ui';
import { KIND_LABEL } from '../vocab';
import { kindVendor } from './Panels';

/** Models offered by name. Anything else can be typed under "Other…". */
export const MODEL_CHOICES: Record<ModelKind, { id: string; label: string }[]> = {
  claude: [
    { id: 'opus', label: 'Opus' },
    { id: 'sonnet', label: 'Sonnet' },
    { id: 'haiku', label: 'Haiku' },
  ],
  codex: [{ id: 'gpt-6-astra', label: 'gpt-6-astra' }],
  // OpenCode's own `provider/model` names, as OpenCode 1.19.4 lists them.
  opencode: [
    { id: 'openrouter/moonshotai/kimi-k2.7-code', label: 'Kimi K2.7 Code' },
    { id: 'openrouter/z-ai/glm-5.3', label: 'GLM-5.3' },
    { id: 'openrouter/deepseek/deepseek-v4-pro', label: 'DeepSeek V4 Pro' },
    { id: 'openrouter/deepseek/deepseek-v4.1-flash', label: 'DeepSeek V4.1 Flash' },
    { id: 'openrouter/openai/gpt-oss-120b', label: 'GPT OSS 120B' },
  ],
};

const OTHER = '__other__';
const INHERIT = '';

export const modelLabel = (k: ModelKind, id: string | null) =>
  id ? (MODEL_CHOICES[k].find((c) => c.id === id)?.label ?? id) : `${KIND_LABEL[k]}'s default`;

type KindSwitchProps = { value: AgentKind | null; onChange: (k: AgentKind) => void; disabled?: boolean; kinds?: AgentKind[]; label: string };

/** Claude / Codex / OpenCode / Practice as one row of buttons; `value` null = mixed. */
/**
 * The kinds offered when adding or switching agents. Practice agents (free, scripted, no model)
 * are no longer offered; a team that already has them keeps them and can switch them.
 */
export const OFFERED_KINDS: AgentKind[] = ['claude', 'codex', 'opencode'];

export function KindSwitch({ value, onChange, disabled, kinds = value === 'practice' ? [...OFFERED_KINDS, 'practice'] : OFFERED_KINDS, label }: KindSwitchProps) {
  return (
    <div className="kind-switch" role="radiogroup" aria-label={label}>
      {kinds.map((k) => (
        <button
          key={k}
          type="button"
          role="radio"
          aria-checked={value === k}
          className={`kind-opt ${kindVendor(k).cls}${value === k ? ' on' : ''}`}
          disabled={disabled}
          onClick={() => onChange(k)}
        >
          <VendorMark v={kindVendor(k)} size="xs" /> {KIND_LABEL[k]}
        </button>
      ))}
    </div>
  );
}

type ModelSelectProps = {
  kind: ModelKind;
  value: string | null;
  /** What "default" means here, e.g. "Sonnet, from the team". */
  inherited: string;
  onChange: (model: string | null) => void;
  disabled?: boolean;
  label: string;
};

/** A model picker: inherit, a named model, or any model id typed under "Other…". */
export function ModelSelect({ kind, value, inherited, onChange, disabled, label }: ModelSelectProps) {
  const id = useId();
  const known = !value || MODEL_CHOICES[kind].some((c) => c.id === value);
  const [typing, setTyping] = useState(!known);
  return (
    <div className="model-select">
      <select
        id={id}
        aria-label={label}
        className="fld-input"
        value={typing ? OTHER : (value ?? INHERIT)}
        disabled={disabled}
        onChange={(e) => {
          setTyping(e.target.value === OTHER);
          if (e.target.value !== OTHER) onChange(e.target.value || null);
        }}
      >
        <option value={INHERIT}>Default · {inherited}</option>
        {MODEL_CHOICES[kind].map((c) => (
          <option key={c.id} value={c.id}>
            {c.label}
          </option>
        ))}
        <option value={OTHER}>Other…</option>
      </select>
      {typing && (
        <input
          className="fld-input mono"
          aria-label={`${label}: model id`}
          placeholder={kind === 'claude' ? 'e.g. claude-opus-5-5' : 'e.g. gpt-6-astra'}
          value={value ?? ''}
          disabled={disabled}
          spellCheck={false}
          onChange={(e) => onChange(e.target.value || null)}
        />
      )}
    </div>
  );
}
