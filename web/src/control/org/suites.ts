import { apiRequest, ApiError } from '../../api';
import type { Team } from '../../tests/types';

/** Arena's list is a definition and run counts; scored figures live in the report. */
type ListedSuite = {
  id: string; name: string; team: Team; tasks: string[]; repeats: number;
  status: 'running' | 'finished' | 'stopped'; runs: number; ended: number; passed: number;
};
type Summary = { runs: number; ended: number; pass_rate: number | null; safety_violations: number };
export type SuiteRow = ListedSuite & { summary?: Summary };
export type Suites = { rows: SuiteRow[] } | { missing: true } | { error: string } | null;

async function withSummary(suite: ListedSuite): Promise<SuiteRow> {
  try {
    const report = await apiRequest<{ summary: Summary }>('GET', `/api/arena/suites/${encodeURIComponent(suite.id)}`);
    return { ...suite, summary: report.summary };
  } catch {
    // Keep the listed suite visible when its report cannot be read; figures stay unknown.
    return suite;
  }
}

export async function readSuites(): Promise<Suites> {
  try {
    const list = await apiRequest<ListedSuite[]>('GET', '/api/arena/suites');
    return { rows: await Promise.all(list.map(withSummary)) };
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) return { missing: true };
    throw e;
  }
}

/** The server redirects to its configured Arena's suite page. */
export const suitePage = (id: string) => `/api/arena/suite-page/${encodeURIComponent(id)}`;

/** Report figures take precedence; absence is never treated as a passing score. */
export function suiteFigures(suite: SuiteRow) {
  const report = suite.summary;
  return {
    ended: report?.ended ?? suite.ended,
    runs: report?.runs ?? suite.runs,
    passRate: report?.pass_rate != null ? `${Math.round(report.pass_rate * 100)}%` : 'unknown',
    safety: report?.safety_violations ?? 'unknown',
    abnormal: !!report?.safety_violations,
  };
}
