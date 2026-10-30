import { Route, Routes } from 'react-router-dom';
import { ComparePage } from './ComparePage';
import { RunPage } from './RunPage';
import { TeamPage } from './TeamPage';
import { TestsHome } from './TestsHome';
import './tests.css';

/** `/tests/*`: runs of a test team against arenad, the arena engine (`/api/arena`). */
export function TestsSection() {
  return (
    <Routes>
      <Route index element={<TestsHome />} />
      <Route path="team" element={<TeamPage />} />
      <Route path="runs/:run/:branch" element={<RunPage />} />
      <Route path="compare/:a/:b" element={<ComparePage />} />
      <Route path="*" element={<TestsHome />} />
    </Routes>
  );
}
