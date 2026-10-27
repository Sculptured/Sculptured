import { useState } from 'react';
import { api, type TriggerEvent } from '../api';
import { errorText } from '../errors';
import { addTrigger, MAX_TRIGGERS, removeTrigger, updateTrigger } from '../draft/triggers';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Trigger } from '../generated/Trigger';
import { Btn } from '../shell/ui';
import { usePolled } from '../usePolling';
import { LOCK_REASON, type Edit } from './Panels';

type Props = { draft: TeamDraft; edit: Edit; locked: boolean };

const TRIGGERS_POLL_MS = 5000;
const clock = (ms: number) => new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

/** What came of an event, as shape and words: ✓ sent, ✗ not delivered, … still being handled. */
export function outcomeWords(e: TriggerEvent): string {
  if (e.outcome === undefined) return '… being handled';
  return `${e.delivered ? '✓' : '✗'} ${e.outcome}`;
}

/**
 * Settings → Events from other systems: each trigger's agent and instructions, its address (with the
 * secret Sculptured keeps), a new address when the old one leaked, and the latest events received.
 */
export function TriggerFields({ draft, edit, locked }: Props) {
  const list = draft.triggers ?? [];
  const { data, reload } = usePolled(() => (list.length ? api.teamTriggers(draft.id) : Promise.resolve(null)), TRIGGERS_POLL_MS, [draft.id, list.map((t) => t.id).join()]);
  return (
    <div className="triggers">
      {list.map((t, i) => (
        <TriggerRow key={t.id} t={t} n={i + 1} draft={draft} edit={edit} locked={locked}
          path={data?.triggers.find((x) => x.id === t.id)?.path ?? null}
          received={(data?.received ?? []).filter((e) => e.trigger === t.id)} onNewAddress={reload} />
      ))}
      <p><Btn size="sm" disabled={locked || list.length >= MAX_TRIGGERS || draft.agents.length === 0} onClick={() => edit((d) => addTrigger(d))}>Add a trigger</Btn></p>
      {locked && <p className="reason">{LOCK_REASON}</p>}
    </div>
  );
}

type RowProps = Props & { t: Trigger; n: number; path: string | null; received: TriggerEvent[]; onNewAddress: () => void };

function TriggerRow({ t, n, draft, edit, locked, path, received, onNewAddress }: RowProps) {
  const set = (patch: Partial<Omit<Trigger, 'id'>>) => edit((d) => updateTrigger(d, t.id, patch), `trigger:${t.id}`);
  const label = `Trigger ${n}`;
  return (
    <div className="trigger" role="group" aria-label={label}>
      <div className="trigger-head">
        <label>Hands the event to{' '}
          <select className="fld-input" aria-label={`${label}: agent`} value={t.agent} disabled={locked} onChange={(e) => set({ agent: e.target.value })}>
            {draft.agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </select>
        </label>
        <Btn size="sm" disabled={locked} aria-label={`Remove trigger ${n}`} onClick={() => edit((d) => removeTrigger(d, t.id))}>Remove</Btn>
      </div>
      <textarea className="fld-input" aria-label={`${label}: what the agent should do with the event`} rows={2} maxLength={4096} disabled={locked}
        placeholder="A machine raised an alarm. Open a work order and find the part." value={t.message} onChange={(e) => set({ message: e.target.value })} />
      <label className="fld-checkline">
        <input type="checkbox" checked={t.starts_team} disabled={locked} onChange={(e) => set({ starts_team: e.target.checked })} />
        Start the team if it isn't running
      </label>
      {path ? <Address teamId={draft.id} trigger={t.id} path={path} onNewAddress={onNewAddress} /> : <p className="muted fld-hint">The address appears once this trigger is saved.</p>}
      {received.length > 0 && <Received label={label} events={received} />}
    </div>
  );
}

/** The full address, Copy, an example call, and a new address when the old one should stop working. */
function Address({ teamId, trigger, path, onNewAddress }: { teamId: string; trigger: string; path: string; onNewAddress: () => void }) {
  const url = `${window.location.origin}${path}`;
  const [copied, setCopied] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [error, setError] = useState('');
  const copy = () => void navigator.clipboard?.writeText(url).then(() => setCopied(true), () => setError('Copy the address by hand.'));
  const replace = () => void api.newTriggerAddress(teamId, trigger).then(() => { setReplacing(false); setCopied(false); onNewAddress(); }, (e: unknown) => setError(errorText(e)));
  return (
    <div className="trigger-address">
      <p className="trigger-url"><code data-raw-evidence aria-label="Address">{url}</code></p>
      <p className="trigger-actions">
        <Btn size="sm" onClick={copy}>{copied ? 'Copied' : 'Copy address'}</Btn>
        {replacing ? <>
          <span className="muted">The current address stops working.</span>
          <Btn size="sm" variant="danger" onClick={replace}>Replace it</Btn>
          <Btn size="sm" onClick={() => setReplacing(false)}>Cancel</Btn>
        </> : <Btn size="sm" onClick={() => setReplacing(true)}>New address</Btn>}
      </p>
      <p className="muted fld-hint">{path.startsWith('/hooks/') ? 'Anyone with this address can hand the team an event, so keep it like a password.' : 'This address works only from this computer. For a system elsewhere, use the hosted app.'}</p>
      <details className="trigger-try"><summary>Try it</summary>
        <pre data-raw-evidence>{`curl -X POST '${url}' \\\n  -H 'content-type: application/json' \\\n  -d '{"machine": "Press 4", "code": "VIB-HIGH"}'`}</pre>
      </details>
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}

function Received({ label, events }: { label: string; events: TriggerEvent[] }) {
  return (
    <ul className="trigger-events" aria-label={`${label}: latest events`}>
      {events.slice(0, 5).map((e) => (
        <li key={e.id}>
          <details><summary><span className="muted">{clock(e.at_ms)}</span> · {outcomeWords(e)}</summary>
            <pre data-raw-evidence>{typeof e.body === 'string' ? e.body : JSON.stringify(e.body, null, 2)}</pre>
          </details>
        </li>
      ))}
    </ul>
  );
}
