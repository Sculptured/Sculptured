import { Navigate, Route, Routes } from 'react-router-dom';
import { useHosted } from '../hosted/HostedGate';
import { AgentDesignGuide } from '../release/AgentDesignGuide';
import { Community } from '../release/Community';
import { BlockPage } from './BlockPage';
import { CommunityLayout } from './CommunityLayout';
import { ExplorePage } from './ExplorePage';
import { ModerationPage } from './ModerationPage';
import { PeoplePage } from './PeoplePage';
import { ProfilePage } from './ProfilePage';
import { PublishPage } from './PublishPage';
import { SavedPage } from './SavedPage';
import { YoursPage } from './YoursPage';
import { YourProfilePage } from './YourProfilePage';

/** `/community/*`. The hosted app signs in to Community with the Sculptured account; the self-run app
 * keeps its own Community page until it can pair with an account. */
export function CommunitySection() {
  const s = useHosted();
  if (s.kind === 'local') return <main className="page page-wide"><Community /></main>;
  return (
    <Routes>
      <Route path="learn" element={<main className="page page-wide"><AgentDesignGuide /></main>} />
      <Route element={<CommunityLayout signedIn={s.kind === 'signed-in'} />}>
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
