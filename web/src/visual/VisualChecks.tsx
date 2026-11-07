import { plural } from '../format';
import { Icon } from '../shell/ui';
import type { Check, Claim } from './process';
import { plainLine, Running } from './visualParts';

function outcomeText(c: Check): string {
  const o = c.outcome;
  if (o.status === 'running') return 'running';
  const exit = o.exit !== null ? ` · exit ${o.exit}` : '';
  return o.status === 'error' ? `✗ error result${exit}` : `✓ no error result${exit}`;
}

function CheckRow({ c, onJump }: { c: Check; onJump: (key: string) => void }) {
  return (
    <li>
      <button type="button" className={`vp-check o-${c.outcome.status}`} onClick={() => onJump(c.key)}>
        <span className="vp-check-top">
          <Icon name="flask" size={13} />
          <code data-raw-evidence>{c.command}</code>
          <span className="vp-badge" title="Kind guessed from the command's words">
            ≈ {c.kind}
          </span>
        </span>
        <span className="vp-check-bottom">
          <span className="tier-inline tier-observed">◆ observed</span>
          <span className={`vp-outcome o-${c.outcome.status}`}>
            {c.outcome.status === 'running' && <Running />}
            {outcomeText(c)}
          </span>
          {c.outcome.mentionsFailure && <span className="tone-amber">· output mentions failures</span>}
          <span className="muted">· turn {c.phase}</span>
        </span>
        {c.outcome.line && (
          <span className="vp-check-line" data-raw-evidence>
            {c.outcome.line}
          </span>
        )}
      </button>
    </li>
  );
}

function Claims({ claims, noChecks, onJump }: { claims: Claim[]; noChecks: boolean; onJump: (key: string) => void }) {
  if (claims.length === 0) return null;
  return (
    <div className="vp-claims">
      <h4>What the agent said</h4>
      {noChecks && <p className="tone-amber vp-claim-warn">The agent reported success, but no check command was seen.</p>}
      <ul>
        {claims.map((c) => (
          <li key={c.key}>
            <button type="button" className="vp-claim" onClick={() => onJump(c.key)}>
              <span className="tier-inline tier-reported">◇ reported</span>
              <span data-raw-evidence>“{plainLine(c.text)}”</span>
              <span className="muted">· turn {c.phase}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function Checks({ checks, claims, onJump }: { checks: Check[]; claims: Claim[]; onJump: (key: string) => void }) {
  return (
    <section className="vp-panel vp-checks" aria-label="Checks">
      <div className="vp-panel-head">
        <h3>Checks</h3>
        <span className="vp-panel-count">{plural(checks.length, 'command')}</span>
      </div>
      <p className="vp-note">
        Commands whose words look like tests, builds, type checks or linters (<span className="tier-inline tier-heuristic">≈ heuristic</span>). Each result is{' '}
        <span className="tier-inline tier-observed">◆ observed</span> from the captured output, and claims nothing more.
      </p>
      {checks.length === 0 ? (
        <p className="muted vp-empty">No test, build or lint commands seen.</p>
      ) : (
        <ul className="vp-check-list">
          {checks.map((c) => <CheckRow key={c.key} c={c} onJump={onJump} />)}
        </ul>
      )}
      <Claims claims={claims} noChecks={checks.length === 0} onJump={onJump} />
    </section>
  );
}
