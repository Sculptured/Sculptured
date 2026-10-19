import { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { errorText } from '../errors';
import type { ProfileReset } from '../generated/community/ProfileReset';
import type { ReportItem } from '../generated/community/ReportItem';
import { Btn } from '../shell/ui';
import { communityApi } from './api';
import { useCommunity } from './context';
import { LoadProblem } from './LoadProblem';
import './community.css';

/** `/community/moderation`: open reports, for Community moderators. */
export function ModerationPage() {
  const { me } = useCommunity();
  if (!me?.admin) return <p>Only Community moderators can see this page.</p>;
  return <Reports />;
}

function Reports() {
  const [reports, setReports] = useState<ReportItem[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const load = useCallback(() => { setError(null); communityApi.reports().then((r) => setReports(r.reports), setError); }, []);
  useEffect(load, [load]);
  if (error) return <LoadProblem error={error} onRetry={load} />;
  if (!reports) return <p role="status" className="muted">Loading reports…</p>;
  if (reports.length === 0) return <p className="muted">No open reports.</p>;
  return (
    <ul className="cm-reports" aria-label="Open reports">
      {reports.map((r) => (
        <li key={r.id}>
          <blockquote data-raw-evidence>{r.reason}</blockquote>
          {r.kind === 'profile' && r.handle ? <ProfileReport handle={r.handle} onDone={load} /> : r.block ? <BlockReport id={r.block.id} title={r.block.title} onDone={load} /> : null}
        </li>
      ))}
    </ul>
  );
}

function BlockReport({ id, title, onDone }: { id: string; title: string; onDone: () => void }) {
  const [problem, setProblem] = useState('');
  const act = async (hidden: boolean) => { try { await communityApi.moderateBlock(id, hidden); onDone(); } catch (e) { setProblem(errorText(e)); } };
  return (
    <div className="cm-actions">
      <Link to={`/community/blocks/${encodeURIComponent(id)}`}>{title}</Link>
      <Btn size="sm" variant="primary" onClick={() => void act(true)}>Hide block</Btn>
      <Btn size="sm" onClick={() => void act(false)}>Keep it and close the report</Btn>
      {problem && <span className="tone-red" role="alert">{problem}</span>}
    </div>
  );
}

const RESETS: [ProfileReset, string][] = [['bio', 'Reset bio'], ['avatar', 'Reset picture'], ['handle', 'Reset handle']];

/** Resets and, only when ticked, disabling: an unticked box never re-enables an account another moderator disabled. */
function ProfileReport({ handle, onDone }: { handle: string; onDone: () => void }) {
  const [reset, setReset] = useState<ProfileReset[]>([]);
  const [disable, setDisable] = useState(false);
  const [problem, setProblem] = useState('');
  const apply = async () => { try { await communityApi.moderateProfile(handle, disable ? { reset, disable: true } : { reset }); onDone(); } catch (e) { setProblem(errorText(e)); } };
  return (
    <div className="cm-actions">
      <Link to={`/community/u/${encodeURIComponent(handle)}`}>@{handle}</Link>
      {RESETS.map(([k, label]) => <label key={k} className="cm-check"><input type="checkbox" checked={reset.includes(k)} onChange={(e) => setReset(e.target.checked ? [...reset, k] : reset.filter((x) => x !== k))} /> {label}</label>)}
      <label className="cm-check"><input type="checkbox" checked={disable} onChange={(e) => setDisable(e.target.checked)} /> Also disable this account</label>
      <Btn size="sm" variant="primary" onClick={() => void apply()}>Apply</Btn>
      {problem && <span className="tone-red" role="alert">{problem}</span>}
    </div>
  );
}
