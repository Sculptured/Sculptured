import { useEffect, useId, useState, type FormEvent } from 'react';
import {
  addConnection, checkConnection, deleteConnection, maskedKey, OPENROUTER_MODEL, PROVIDER_LABEL, useConnections,
  type ConnectionCheck, type ConnectionProvider, type ModelConnection, type NewConnection,
} from '../hosted/connections';
import { errorText } from '../errors';
import { GwError } from '../hosted/session';

/** Plain words for a failed request: the gateway's own messages are written for people; the core's go through errorText. */
export const failText = (e: unknown) => (e instanceof GwError ? e.message : errorText(e));

/**
 * Pay-per-use model connections: the saved ones with Check and Delete, and a form for each kind.
 * A key is sent once, kept sealed on the server, and only its last four characters come back.
 * `onWorking` says whether any connection's latest check passed.
 */
export function ModelConnections({ onWorking }: { onWorking?: (ok: boolean) => void }) {
  const { list, loaded, reload } = useConnections(true);
  const [results, setResults] = useState<Record<string, ConnectionCheck>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');
  const checkOf = (c: ModelConnection) => results[c.id] ?? c.checked;
  const anyOk = list.some((c) => checkOf(c)?.ok === true);
  useEffect(() => { if (loaded) onWorking?.(anyOk); }, [anyOk, loaded]); // eslint-disable-line react-hooks/exhaustive-deps

  const check = async (id: string) => {
    setBusy(id); setError('');
    try {
      const r = await checkConnection(id);
      setResults((m) => ({ ...m, [id]: { ok: r.ok, detail: r.detail, at: new Date().toISOString() } }));
      await reload();
    } catch (e) { setError(failText(e)); } finally { setBusy(null); }
  };
  const remove = async (id: string) => {
    setBusy(id); setError('');
    try { await deleteConnection(id); await reload(); } catch (e) { setError(failText(e)); } finally { setBusy(null); }
  };

  return (
    <>
      <h3>Pay-per-use keys</h3>
      <p className="muted">A key is kept sealed on the Sculptured server, handed to an agent only when it starts, and never shown again. Runs with a key are billed to your own Anthropic or OpenRouter account. Check asks the provider whether the key works and uses no tokens.</p>
      {list.length > 0 && (
        <ul className="setup-list" aria-label="Your model connections">
          {list.map((c) => <ConnectionRow key={c.id} c={c} check={checkOf(c)} busy={busy === c.id} onCheck={() => void check(c.id)} onDelete={() => void remove(c.id)} />)}
        </ul>
      )}
      {loaded && list.length === 0 && <p className="muted">No keys yet.</p>}
      {error && <p className="tone-red" role="alert">{error}</p>}
      <div className="setup-forms">
        <KeyForm provider="anthropic-key" onAdded={async (c) => { await reload(); await check(c.id); }} />
        <KeyForm provider="openrouter" onAdded={async (c) => { await reload(); await check(c.id); }} />
      </div>
    </>
  );
}

function CheckState({ check }: { check: ConnectionCheck | null }) {
  if (!check) return <span className="muted"><span aria-hidden="true">○ </span>Not checked yet</span>;
  const when = new Date(check.at).toLocaleString();
  if (check.ok) return <span className="tone-green"><span aria-hidden="true">✓ </span>Works · checked {when}</span>;
  return (
    <span className="tone-red">
      <span aria-hidden="true">⚠ </span>Check failed · {when}{check.detail && <>: <span data-raw-evidence>{check.detail}</span></>}
    </span>
  );
}

function ConnectionRow({ c, check, busy, onCheck, onDelete }: { c: ModelConnection; check: ConnectionCheck | null; busy: boolean; onCheck: () => void; onDelete: () => void }) {
  return (
    <li aria-label={c.name}>
      <strong>{c.name}</strong>
      <span className="muted">{PROVIDER_LABEL[c.provider]}{c.model ? <> · <code>{c.model}</code></> : null}</span>
      <span className="mono" aria-label={`Key ending ${c.key_last4}`}>{maskedKey(c.key_last4)}</span>
      <CheckState check={check} />
      <span className="setup-row-actions">
        <button type="button" className="btn btn-sm" disabled={busy} aria-label={`Check ${c.name}`} onClick={onCheck}>{busy ? 'Checking…' : 'Check'}</button>
        <button type="button" className="btn btn-sm" disabled={busy} aria-label={`Delete ${c.name}`} onClick={onDelete}>Delete</button>
      </span>
    </li>
  );
}

const FORM: Record<ConnectionProvider, { title: string; blurb: string; keyLabel: string; name: string }> = {
  'anthropic-key': { title: 'Claude — API key', blurb: 'An Anthropic API key, from console.anthropic.com.', keyLabel: 'Anthropic API key', name: 'Anthropic key' },
  openrouter: { title: 'Claude via OpenRouter', blurb: 'An OpenRouter key and the model Claude Code should use.', keyLabel: 'OpenRouter key', name: 'OpenRouter' },
};

function KeyForm({ provider, onAdded }: { provider: ConnectionProvider; onAdded: (c: ModelConnection) => Promise<void> }) {
  const ids = useId();
  const f = FORM[provider];
  const [name, setName] = useState('');
  const [key, setKey] = useState('');
  const [model, setModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    if (!key.trim()) { setError('Paste the key first.'); return; }
    if (provider === 'openrouter' && !model.trim()) { setError(`Choose a model, for example ${OPENROUTER_MODEL}.`); return; }
    const body: NewConnection = { name: name.trim() || f.name, provider, key: key.trim() };
    if (provider === 'openrouter') body.model = model.trim();
    setBusy(true);
    try {
      const c = await addConnection(body);
      // The key leaves the page as soon as the server has it.
      setKey(''); setName(''); setModel('');
      await onAdded(c);
    } catch (err) { setError(failText(err)); } finally { setBusy(false); }
  };
  return (
    <form className="setup-form" aria-label={f.title} noValidate onSubmit={(e) => void submit(e)}>
      <h3>{f.title}</h3>
      <p className="muted">{f.blurb}</p>
      <label className="fld-label" htmlFor={`${ids}-name`}>Name</label>
      <input id={`${ids}-name`} className="fld-input" value={name} placeholder={f.name} onChange={(e) => setName(e.target.value)} />
      <label className="fld-label" htmlFor={`${ids}-key`}>{f.keyLabel}</label>
      <input id={`${ids}-key`} className="fld-input" type="password" autoComplete="off" spellCheck={false} value={key} onChange={(e) => setKey(e.target.value)} />
      {provider === 'openrouter' && (
        <>
          <label className="fld-label" htmlFor={`${ids}-model`}>Model</label>
          <input id={`${ids}-model`} className="fld-input mono" value={model} placeholder={OPENROUTER_MODEL} spellCheck={false} aria-describedby={`${ids}-mhint`} onChange={(e) => setModel(e.target.value)} />
          <p id={`${ids}-mhint`} className="fld-hint">Claude models work best: Claude Code is made for them, and other models may not handle its tools.</p>
        </>
      )}
      {error && <p className="tone-red" role="alert">{error}</p>}
      <p><button type="submit" className="btn btn-primary" disabled={busy}>{busy ? 'Saving…' : 'Save and check'}</button></p>
    </form>
  );
}
