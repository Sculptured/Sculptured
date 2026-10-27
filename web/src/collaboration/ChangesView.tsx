import { api, type ProposedChangeRow } from '../api';
import { ChangeFields, RuleResults } from '../shell/ChangeApproval';
import { usePolled } from '../usePolling';
import { shortAge } from '../herdr';
import './status-page.css';

const ago = (ms: number, now: number) => {
  const a = shortAge(ms, now);
  return a === 'now' ? 'just now' : `${a} ago`;
};

/** How often the list is read again while open: approvals and decisions arrive as the team works. */
export const CHANGES_POLL_MS = 3000;

/** What became of a change, as shape and words: who decided, and why when not approved. */
export function decisionWords(c: ProposedChangeRow): { mark: string; words: string; tone: string } {
  const d = c.decision;
  if (!d) return { mark: '◌', words: 'Waiting for a person in Needs you', tone: 'tone-amber' };
  if (d.approved) return { mark: '✓', words: d.by === 'mandate' ? "Approved by the team's rules" : 'Approved by a person', tone: 'tone-green' };
  if (d.by === 'nobody-in-time') return { mark: '✗', words: 'Not approved: nobody answered in time', tone: 'tone-red' };
  return { mark: '✗', words: d.note ? `Not approved: ${d.note}` : 'Not approved by a person', tone: 'tone-red' };
}

function ChangeRow({ c, now }: { c: ProposedChangeRow; now: number }) {
  const w = decisionWords(c);
  return (
    <li className="change-row">
      <p className="change-head">
        <strong data-raw-evidence>{c.summary}</strong>
        <span className="muted"> · {c.agent_name} · {c.action}{c.system ? ` · ${c.system}` : ''} · {ago(c.at_ms, now)}</span>
      </p>
      <p className={w.tone}><span aria-hidden="true">{w.mark} </span>{w.words}</p>
      <details>
        <summary>The change and its rules</summary>
        <p className="muted">Proposed by the agent (agent-reported); the rules are checked by Sculptured.</p>
        <ChangeFields payload={c.payload} />
        <RuleResults checks={c.checks ?? []} />
        {c.reason && <p><span className="muted">Its reason: </span><span data-raw-evidence>{c.reason}</span></p>}
      </details>
    </li>
  );
}

/**
 * Results → Changes: every change the team's agents proposed to a business system, newest first, with the
 * team's rules as Sculptured checked them and what became of it. Kept after the team stops.
 */
export function ChangesView({ rig }: { rig: string }) {
  const { data, error } = usePolled(() => api.teamChanges(rig), CHANGES_POLL_MS, [rig]);
  const now = Date.now();
  return (
    <section className="changes-view" aria-label="Changes">
      <h2>Changes</h2>
      {error && <p role="alert">{error}</p>}
      {!data && !error && <p className="muted">Loading…</p>}
      {data && data.changes.length === 0 && (
        <p className="muted">No changes proposed yet. Agents propose each change to an order, a schedule or another system with propose_change; set the rules they must meet under Settings → Changes to business systems.</p>
      )}
      {data && data.changes.length > 0 && <ul className="changes-list">{data.changes.map((c) => <ChangeRow key={c.id} c={c} now={now} />)}</ul>}
    </section>
  );
}
