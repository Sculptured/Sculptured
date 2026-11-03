import { useId } from 'react';
import { modelPlaceholder, modelProblem, providerOptions, providerState } from './providers';
import type { ProviderStatus } from './types';

/** The provider select of an API-mode agent; unavailable providers are disabled and say why. */
export function ProviderSelect({ value, list, onChange }: { value: string; list: ProviderStatus[]; onChange: (id: string) => void }) {
  return (
    <label className="tst-field">
      <span className="tst-label">Provider</span>
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {providerOptions(list, value).map((o) => (
          <option key={o.id} value={o.id} disabled={o.disabled}>{o.text}</option>
        ))}
      </select>
    </label>
  );
}

type ModelProps = {
  /** The agent's provider; null for a CLI agent (no list, the CLI's default). */
  provider: string | null;
  value: string;
  list: ProviderStatus[];
  onChange: (model: string) => void;
};

/** A model box: the provider's listed models as suggestions, or any id typed in. */
export function ModelInput({ provider, value, list, onChange }: ModelProps) {
  const listId = useId();
  const models = (provider && list.find((s) => s.id === provider)?.models) || [];
  return (
    <label className="tst-field tst-grow">
      <span className="tst-label">Model</span>
      <input className="mono" list={models.length ? listId : undefined} value={value}
        placeholder={provider ? modelPlaceholder(provider) : 'default for this agent'} onChange={(e) => onChange(e.target.value)} />
      {models.length > 0 && <datalist id={listId}>{models.map((m) => <option key={m} value={m} />)}</datalist>}
    </label>
  );
}

/** Under the provider and model: why the agent can't run, or where its models come from. */
export function ModelHint({ provider, model, list }: { provider: string; model: string | null | undefined; list: ProviderStatus[] }) {
  const problem = modelProblem(provider, model, list);
  if (problem) return <p className="tst-small" role="status"><span aria-hidden="true">⚠</span> {problem}</p>;
  const s = list.find((x) => x.id === provider);
  if (!s) return null;
  if (s.models?.length) return <p className="muted tst-small">{s.models.length} models listed by {s.label}; or type any model id.</p>;
  if (providerState(s) === 'not checked') return <p className="muted tst-small">{s.label} not checked yet; type a model id.</p>;
  if (s.why) return <p className="muted tst-small">Models not listed ({s.why}); type a model id.</p>;
  if (s.kind === 'openrouter') return <p className="muted tst-small">Any model id from openrouter.ai/models. Every model call is recorded.</p>;
  return null;
}
