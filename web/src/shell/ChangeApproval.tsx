import type { QuestionView } from '../generated/QuestionView';
import type { RuleResult } from '../generated/RuleResult';

/** What `propose_change` puts in an approval's `tool_input`. */
export type ProposedChange = {
  proposal_id?: string;
  system?: string | null;
  payload?: Record<string, unknown>;
  reason?: string | null;
  checks?: RuleResult[];
};

export const isChange = (q: Pick<QuestionView, 'source'>) => q.source === 'approval';

const shown = (v: unknown) => (typeof v === 'string' ? v : JSON.stringify(v));

/** The team's rules as Sculptured checked them: shape and words, never colour alone. */
export function RuleResults({ checks }: { checks: RuleResult[] }) {
  if (checks.length === 0) return <p className="muted">No team rule applies to this change, so a person decides.</p>;
  return (
    <ul className="change-checks" aria-label="The team's rules (checked by Sculptured)">
      {checks.map((c) => (
        <li key={c.rule_id} className={c.passed ? 'tone-green' : 'tone-red'}>
          <span aria-hidden="true">{c.passed ? '✓ ' : '✗ '}</span>
          <span className="sr-only">{c.passed ? 'passes: ' : 'fails: '}</span>
          {c.words}
        </li>
      ))}
    </ul>
  );
}

/** The change's fields, as the agent would write them. */
export function ChangeFields({ payload }: { payload: Record<string, unknown> }) {
  const rows = Object.entries(payload);
  if (rows.length === 0) return null;
  return (
    <table className="change-fields" aria-label="The change, as it would be written">
      <tbody>
        {rows.map(([k, v]) => (
          <tr key={k}><th scope="row">{k}</th><td data-raw-evidence>{shown(v)}</td></tr>
        ))}
      </tbody>
    </table>
  );
}

/** An approval in Needs you: the change, its rule results and the agent's reason (agent-reported). */
export function ChangeDetails({ q }: { q: QuestionView }) {
  const c = (q.prompt.tool_input ?? {}) as ProposedChange;
  return (
    <div className="change-details">
      <p className="muted">
        {q.prompt.tool ?? 'A change'}{c.system ? ` · ${c.system}` : ''} · proposed by the agent (agent-reported); the rules below are checked by Sculptured
      </p>
      <ChangeFields payload={c.payload ?? {}} />
      <RuleResults checks={c.checks ?? []} />
      {c.reason && <p><span className="muted">Its reason: </span><span data-raw-evidence>{c.reason}</span></p>}
    </div>
  );
}
