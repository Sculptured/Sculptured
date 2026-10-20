import { useEffect, useId, useState } from 'react';
import { api, type TraceExportView } from '../api';
import { Btn } from '../shell/ui';
import { useHosted } from '../hosted/HostedGate';

const LAMINAR = 'https://api.lmnr.ai/v1/traces';
type Set = (change: Partial<TraceExportView>) => void;

/** The saved setting, the form being edited, and saving (no key keeps the saved one, '' removes it). */
function useTraces(teamId: string) {
  const [saved, setSaved] = useState<TraceExportView | null>(null);
  const [form, setForm] = useState<TraceExportView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    api.traces(teamId).then((t) => { setSaved(t); setForm(t); }, (e: Error) => setError(e.message));
    // "Last sent" and the last error change as the exporter works; edits in the form are kept.
    const every = setInterval(() => api.traces(teamId).then(setSaved, () => {}), 30_000);
    return () => clearInterval(every);
  }, [teamId]);
  /** Saves `from` (the form, or the saved setting for Remove key) with this endpoint and key. */
  const save = async (from: TraceExportView | null, endpoint: string, key: string | undefined) => {
    if (!from) return false;
    setBusy(true);
    setError('');
    try {
      const t = await api.saveTraces(teamId, { enabled: from.enabled, endpoint, content: from.content, ...(key === undefined ? {} : { key }) });
      setSaved(t);
      setForm(t);
      return true;
    } catch (e) {
      setError((e as Error).message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  const set: Set = (change) => setForm((f) => (f ? { ...f, ...change } : f));
  return { saved, form, set, save, busy, error };
}

/**
 * Send traces: this team's runs as OpenTelemetry traces to Laminar or another OTLP/HTTP collector.
 * The key is kept by the server; after saving, the page only knows that one is saved.
 */
export function TraceSettings({ teamId }: { teamId: string }) {
  const t = useTraces(teamId);
  const [key, setKey] = useState('');
  const [other, setOther] = useState<boolean | null>(null);
  if (!t.form || !t.saved) return t.error ? <p className="tone-red" role="alert">{t.error}</p> : null;
  const { form, saved } = t;
  const isOther = other ?? form.endpoint !== LAMINAR;
  const save = (k: string | undefined) => void t.save(form, isOther ? form.endpoint.trim() : LAMINAR, k).then((ok) => ok && setKey(''));
  const removeKey = () => void t.save(saved, saved.endpoint, '');
  return (
    <fieldset className="ta-limits">
      <legend className="fld-label">Send traces</legend>
      <label className="ta-check">
        <input type="checkbox" checked={form.enabled} onChange={(e) => t.set({ enabled: e.target.checked })} />
        <span><b>Send this team's runs as traces</b><span className="fld-meta">Each run becomes one OpenTelemetry trace: the run, each agent’s session, its turns (model, tokens, cost) and tool calls. For people who already use an observability tool; the board stays the main view.</span></span>
      </label>
      <Destination form={form} other={isOther} setOther={setOther} set={t.set} />
      <KeyField saved={saved.key_saved} value={key} onChange={setKey} onRemove={removeKey} busy={t.busy} />
      <ContentField form={form} set={t.set} />
      <div className="set-links">
        <Btn onClick={() => save(key.trim() ? key : undefined)} disabled={t.busy}>Save</Btn>
        <SendStatus saved={saved} />
      </div>
      {saved.last_error && <p className="tone-red">Last send failed: {saved.last_error}</p>}
      {t.error && <p className="tone-red" role="alert">{t.error}</p>}
    </fieldset>
  );
}

function Destination({ form, other, setOther, set }: { form: TraceExportView; other: boolean; setOther: (o: boolean) => void; set: Set }) {
  const ids = useId();
  const hosted = useHosted().kind === 'signed-in';
  const choose = (o: boolean) => { setOther(o); if (o && form.endpoint === LAMINAR) set({ endpoint: '' }); };
  return (
    <>
      <div className="fld">
        <label className="fld-label" htmlFor={`${ids}-to`}>Send to</label>
        <select id={`${ids}-to`} className="fld-input" value={other ? 'other' : 'laminar'} onChange={(e) => choose(e.target.value === 'other')}>
          <option value="laminar">Laminar</option>
          <option value="other">Another OTLP/HTTP collector</option>
        </select>
      </div>
      {other && (
        <div className="fld">
          <label className="fld-label" htmlFor={`${ids}-url`}>Collector address</label>
          <input id={`${ids}-url`} className="fld-input" value={form.endpoint} placeholder="https://collector.example.com/v1/traces" spellCheck={false} onChange={(e) => set({ endpoint: e.target.value })} />
          <p className="fld-hint">{hosted ? 'A public https:// address.' : 'An https:// address, or http:// to this computer.'}</p>
        </div>
      )}
    </>
  );
}

function KeyField({ saved, value, onChange, onRemove, busy }: { saved: boolean; value: string; onChange: (v: string) => void; onRemove: () => void; busy: boolean }) {
  const id = useId();
  return (
    <div className="fld">
      <label className="fld-label" htmlFor={id}>API key</label>
      <input id={id} className="fld-input" type="password" autoComplete="off" value={value} placeholder={saved ? 'Type a new key to replace the saved one' : ''} onChange={(e) => onChange(e.target.value)} />
      <p className="fld-hint">
        {saved ? 'A key is saved. ' : ''}Kept by Sculptured’s server and sent only to this address; it is never shown again. A new address needs its key again.
        {saved && <> <Btn onClick={onRemove} disabled={busy}>Remove key</Btn></>}
      </p>
    </div>
  );
}

function ContentField({ form, set }: { form: TraceExportView; set: Set }) {
  const name = useId();
  return (
    <div className="fld" role="radiogroup" aria-label="What to send">
      <span className="fld-label">What to send</span>
      <label className="ta-check"><input type="radio" name={name} checked={form.content === 'metadata'} onChange={() => set({ content: 'metadata' })} /><span>Timings, tokens and costs</span></label>
      <label className="ta-check"><input type="radio" name={name} checked={form.content === 'full'} onChange={() => set({ content: 'full' })} /><span>Also prompts and outputs (each capped at 4 KB)</span></label>
    </div>
  );
}

function SendStatus({ saved }: { saved: TraceExportView }) {
  const text = saved.last_error ? '' : saved.last_sent_ms ? `Last sent ${new Date(saved.last_sent_ms).toLocaleString()}` : saved.enabled ? 'Nothing sent yet' : '';
  return <span className="muted" role="status">{text}</span>;
}
