import { useOrg } from './data';
import { suiteFigures, suitePage } from './suites';
import './suites.css';

export const SUITES_MISSING = 'Test suites are not available in this Arena yet. They show here, scenario by scenario, once Arena has suites.';

export function SuitesPane() {
  const { suites } = useOrg();
  if (!suites) return <p className="ctl-empty">Reading Arena's test suites…</p>;
  if ('missing' in suites) return <p className="ctl-empty">○ {SUITES_MISSING}</p>;
  if ('error' in suites) return <p className="ctl-empty">Arena's test suites could not be read: {suites.error}</p>;
  if (suites.rows.length === 0) return <p className="ctl-empty">No test suites yet.</p>;
  return (
    <table className="ctl-table org-suites" aria-label="Test suites">
      <thead><tr><th scope="col">Suite</th><th scope="col">Team</th><th scope="col">Status</th><th scope="col" className="ctl-num">Runs done / total</th><th scope="col" className="ctl-num">Pass rate</th><th scope="col" className="ctl-num">Safety violations</th></tr></thead>
      <tbody>
        {suites.rows.map((s) => {
          const figures = suiteFigures(s);
          return <tr key={s.id} className={figures.abnormal ? 'is-abnormal' : undefined}>
            <td data-label="Suite" className="ctl-mono"><a href={suitePage(s.id)}>{s.name}</a></td>
            <td data-label="Team">{s.team.name}</td>
            <td data-label="Status">{s.status === 'running' ? '⣟' : s.status === 'finished' ? '■' : '⊘'} {s.status}</td>
            <td data-label="Runs done / total" className="ctl-num">{figures.ended} / {figures.runs}</td>
            <td data-label="Pass rate" className="ctl-num">{figures.passRate}</td>
            <td data-label="Safety violations" className={`ctl-num${figures.abnormal ? ' pl-abn' : ''}`}>{figures.safety}</td>
          </tr>;
        })}
      </tbody>
    </table>
  );
}
