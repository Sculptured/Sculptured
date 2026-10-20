import { useState, type ReactElement } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import { useHosted } from '../hosted/HostedGate';
import { AgentDesignGuide } from '../release/AgentDesignGuide';
import { Btn } from '../shell/ui';
import { BlockPage } from './BlockPage';
import { CommunityLayout } from './CommunityLayout';
import { ExplorePage } from './ExplorePage';
import { useCommunityLink } from './link';
import { ModerationPage } from './ModerationPage';
import { PeoplePage } from './PeoplePage';
import { ProfilePage } from './ProfilePage';
import { PublishPage } from './PublishPage';
import { SavedPage } from './SavedPage';
import { KeyLeftNote } from './KeyLeftNote';
import { TurnOnCommunity } from './TurnOnCommunity';
import { YoursPage } from './YoursPage';
import { YourProfilePage } from './YourProfilePage';

/** `/community/*`, the same pages in every build. The hosted app signs in with the Sculptured
 * account; on your own computer Community stays off (and silent) until you turn it on. */
export function CommunitySection() {
  const s = useHosted();
  return (
    <Routes>
      <Route path="learn" element={<main className="page page-wide"><AgentDesignGuide /></main>} />
      <Route path="*" element={s.kind === 'local' ? <OnThisComputer /> : <Pages layout={<CommunityLayout signedIn={s.kind === 'signed-in'} workspace={s.kind === 'signed-in'} />} />} />
    </Routes>
  );
}

function Pages({ layout }: { layout: ReactElement }) {
  return (
    <Routes>
      <Route element={layout}>
        <Route index element={<ExplorePage />} />
        <Route path="people" element={<PeoplePage />} />
        <Route path="u/:handle" element={<ProfilePage />} />
        <Route path="me" element={<YourProfilePage />} />
        <Route path="saved" element={<SavedPage />} />
        <Route path="mine" element={<YoursPage />} />
        <Route path="publish" element={<PublishPage />} />
        <Route path="moderation" element={<ModerationPage />} />
        <Route path="blocks/:id" element={<BlockPage />} />
        <Route path="*" element={<Navigate replace to="/community" />} />
      </Route>
    </Routes>
  );
}

/** This computer: the link first (nothing else is asked until it is read), then Turn on, then the pages. */
function OnThisComputer() {
  const { link, error, refresh } = useCommunityLink();
  const [askSignIn, setAskSignIn] = useState(false);
  if (!link) return <main className="page page-wide community">{error ? <LinkProblem error={error} onRetry={() => void refresh()} /> : <p role="status" className="muted">Loading Community…</p>}</main>;
  if (!link.enabled) {
    return (
      <main className="page page-wide community" data-vocab="community">
        <KeyLeftNote />
        <TurnOnCommunity suggested={link.suggested_origin} onDone={(signIn) => { setAskSignIn(signIn); void refresh(); }} />
      </main>
    );
  }
  return <Pages layout={<CommunityLayout signedIn={link.signed_in} workspace signInOpen={askSignIn} />} />;
}

function LinkProblem({ error, onRetry }: { error: string; onRetry: () => void }) {
  return <div className="cm-problem" role="alert"><p>{error}</p><Btn size="sm" onClick={onRetry}>Retry</Btn></div>;
}
