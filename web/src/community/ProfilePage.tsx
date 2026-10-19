import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import type { UserPage } from '../generated/community/UserPage';
import { communityApi } from './api';
import { LoadProblem } from './LoadProblem';
import { ProfileView } from './ProfileView';
import { ReportForm } from './ReportForm';
import './community.css';

/** `/community/u/:handle`: a member's profile; an old handle moves to the current one. */
export function ProfilePage() {
  const { handle = '' } = useParams();
  const navigate = useNavigate();
  const [page, setPage] = useState<UserPage | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => { setError(null); setPage(null); communityApi.profile(handle).then(setPage, setError); }, [handle, attempt]);
  useEffect(() => {
    if (page && page.profile.handle !== handle) navigate(`/community/u/${encodeURIComponent(page.profile.handle)}`, { replace: true });
  }, [page, handle, navigate]);
  if (error) return <LoadProblem error={error} onRetry={() => setAttempt((n) => n + 1)} />;
  if (!page) return <p role="status" className="muted">Loading profile…</p>;
  return (
    <>
      <ProfileView {...page} />
      <ReportForm what="profile" send={(reason) => communityApi.reportProfile(page.profile.handle, reason)} />
    </>
  );
}
