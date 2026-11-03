import { Navigate, Route, Routes, useLocation } from 'react-router-dom';
import { ComparePage } from '../tests/ComparePage';
import { RunPage } from '../tests/RunPage';
import { ArenaControlPage } from './ArenaControlPage';
import { ARENA_PATH, legacyTestsPath } from './paths';
import '../tests/tests.css';

/** `/arena/*`: Arena control, a branch's page and a comparison. Pop-out panes are routed outside the app's bars (App.tsx). */
export function ArenaSection() {
  return (
    <Routes>
      <Route index element={<ArenaControlPage />} />
      <Route path="runs/:run/:branch" element={<RunPage />} />
      <Route path="compare/:a/:b" element={<ComparePage />} />
      <Route path="*" element={<Navigate replace to={ARENA_PATH} />} />
    </Routes>
  );
}

/** `/tests/*`, the old Tests section: each address goes to the matching Arena one. */
export function TestsRedirect() {
  const { pathname, search } = useLocation();
  return <Navigate replace to={legacyTestsPath(pathname, search)} />;
}
