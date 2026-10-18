import { BrowserRouter, Navigate, Route, Routes, useLocation, useParams } from 'react-router-dom';
import { ReleasePage } from './release/ReleasePage';
import { Profile } from './release/Profile';
import { Community } from './release/Community';
import { GitPage } from './release/GitPage';
import { hosted } from './release/Nav';
import { Shell } from './shell/Shell';
import type { OpenStream } from './useRigStream';
import { HostedGate, useHosted } from './hosted/HostedGate';
import { JoinPage } from './hosted/JoinPage';
import { LoginPage } from './hosted/LoginPage';
import { ResetPage } from './hosted/ResetPage';
import { AdminPage } from './hosted/AdminPage';
import { ConnectPage } from './hosted/ConnectPage';
import { ComputersPage } from './hosted/ComputersPage';
import { AccountPage } from './account/AccountPage';
import { OpsPage } from './hosted/OpsPage';
import { WhatWeRecord } from './hosted/WhatWeRecord';
import { LibraryRoute } from './shell/LibraryPage';
import { agentPagePath, agentTabOf, boardPath, resultsPath } from './paths';
import { ProjectsHome } from './pages/ProjectsHome';
import { NewProject } from './pages/NewProject';
import { ProjectLayout } from './pages/ProjectLayout';
import { BoardPage } from './pages/BoardPage';
import { AgentsPage } from './pages/AgentsPage';
import { AgentPage } from './pages/AgentPage';
import { ResultsPage } from './pages/ResultsPage';
import { SettingsPage } from './pages/SettingsPage';
import { WorkflowPage } from './pages/WorkflowPage';

/** Hosted beta admin page; anyone else goes home. */
function AdminGuard() {
  const s = useHosted();
  return s.kind === 'signed-in' && s.user.role === 'admin' ? <AdminPage /> : <Navigate replace to="/" />;
}

/** Old addresses keep working: each goes to the page that now does its job. */
function Legacy({ to }: { to: 'board' | 'design' | 'results' | 'agent' | 'git' }) {
  const { team, rig, seat, tab, '*': rest } = useParams();
  const { search } = useLocation();
  const id = (team ?? rig)!;
  if (to === 'results') return <Navigate replace to={resultsPath(id)} />;
  if (to === 'git') return <Navigate replace to={`/p/${encodeURIComponent(id)}/git`} />;
  if (to === 'design') return <Navigate replace to={boardPath(id, 'design')} />;
  if (to === 'agent') {
    const agent = seat ?? rest?.split('/')[0];
    const t = agentTabOf(tab ?? rest?.split('/')[1]);
    if (!agent) return <Navigate replace to={boardPath(id)} />;
    return <Navigate replace to={agentPagePath(id, agent, t.tab) + (t.search || search)} />;
  }
  // `/rig/:rig?view=history` was the Run board's history; it is part of Results now.
  if (new URLSearchParams(search).get('view') === 'history') return <Navigate replace to={resultsPath(id, 'history')} />;
  return <Navigate replace to={boardPath(id)} />;
}

function ProjectRoutes() {
  return (
    <Route path="/p/:team" element={<ProjectLayout />}>
      <Route index element={<Navigate replace to="board" />} />
      <Route path="board" element={<BoardPage />} />
      <Route path="agents" element={<AgentsPage />} />
      <Route path="agents/:agent/:tab?" element={<AgentPage />} />
      <Route path="results" element={<ResultsPage />} />
      <Route path="results/:section" element={<ResultsPage />} />
      <Route path="settings" element={<SettingsPage />} />
      <Route path="workflow/:target" element={<WorkflowPage />} />
      <Route path="git" element={<main className="page page-wide git-host"><GitPage /></main>} />
    </Route>
  );
}

export function AppRoutes({ openStream }: { openStream?: OpenStream }) {
  if (hosted)
    return (
      <Routes>
        <Route path="/community" element={<ReleasePage><Profile /></ReleasePage>} />
        <Route path="/community/*" element={<ReleasePage><Community /></ReleasePage>} />
        <Route path="*" element={<Navigate replace to="/community" />} />
      </Routes>
    );
  return (
    <Routes>
      <Route path="/join/:token" element={<JoinPage />} />
      <Route path="/login" element={<LoginPage />} />
      <Route path="/reset/:token" element={<ResetPage />} />
      <Route path="/admin" element={<AdminGuard />} />
      <Route path="/connect" element={<ConnectPage />} />
      <Route path="/computers" element={<ComputersPage />} />
      <Route path="/ops" element={<OpsPage />} />
      <Route path="/what-we-record" element={<WhatWeRecord />} />
      <Route element={<Shell openStream={openStream} />}>
        <Route path="/" element={<ProjectsHome />} />
        <Route path="/new" element={<NewProject />} />
        <Route path="/account" element={<AccountPage />} />
        <Route path="/community/*" element={<main className="page page-wide"><Community /></main>} />
        <Route path="/library/:collection" element={<LibraryRoute />} />
        {ProjectRoutes()}
      </Route>
      <Route path="/library" element={<Navigate replace to="/library/groups" />} />
      <Route path="/team/:team/structure" element={<Legacy to="design" />} />
      <Route path="/team/:team/progress" element={<Legacy to="results" />} />
      <Route path="/team/:team/git" element={<Legacy to="git" />} />
      <Route path="/team/:team/live" element={<Legacy to="board" />} />
      <Route path="/team/:team" element={<Legacy to="board" />} />
      <Route path="/rig/:rig" element={<Legacy to="board" />} />
      <Route path="/rig/:rig/agent/:seat" element={<Legacy to="agent" />} />
      <Route path="/rig/:rig/agent/:seat/:tab" element={<Legacy to="agent" />} />
      <Route path="/rig/:rig/seat/*" element={<Legacy to="agent" />} />
      <Route path="*" element={<Navigate replace to="/" />} />
    </Routes>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      {hosted ? <AppRoutes /> : <HostedGate><AppRoutes /></HostedGate>}
    </BrowserRouter>
  );
}
