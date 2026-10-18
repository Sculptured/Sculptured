import { useEffect, useMemo, useState } from 'react';
import { Outlet, useParams } from 'react-router-dom';
import { liveIssues } from '../draft/issues';
import { useShell } from '../shell/ShellContext';
import { hasProcess } from '../status';
import { useTeamDraft } from '../useTeamDraft';
import { ProjectBar } from './ProjectBar';
import { ProjectContext, type Project } from './ProjectContext';

/** `/p/:team`: the project tab row, then the page. A new project starts afresh (its own design and undo). */
export function ProjectLayout() {
  const { team } = useParams();
  return <ProjectScope key={team} team={team!} />;
}

function ProjectScope({ team }: { team: string }) {
  const shell = useShell();
  const t = useTeamDraft(team);
  const { onDraft } = shell;
  useEffect(() => { if (t.draft) onDraft(t.draft); }, [t.draft, onDraft]);
  const stream = shell.teams[team];
  const live = stream?.state.snapshot ? stream.state : null;
  const summary = shell.summaries?.find((s) => s.id === team);
  const builtHere = !!t.draft || !!summary || (!!shell.drafts[team] && !t.loadError);
  const running = live ? Object.values(live.seats).some((s) => hasProcess(s.lifecycle)) : t.serverLive;
  const issues = useMemo(() => (t.draft ? liveIssues(t.draft, t.issues) : []), [t.draft, t.issues]);
  const row = shell.rows.find((r) => r.rig === team);
  const [startBlock, setStartBlock] = useState<string | null>(null);
  const project: Project = {
    team,
    title: t.draft?.name.trim() || shell.titles[team] || row?.title || team,
    t,
    draft: t.draft,
    issues,
    builtHere,
    started: !!live || !!summary?.started || !!shell.rigs?.some((r) => r.name === team) || t.startedRevision !== null,
    live,
    connected: !!stream?.connected,
    running,
    row,
    startBlock,
    setStartBlock,
  };
  return (
    <ProjectContext.Provider value={project}>
      <div className="project-frame">
        <ProjectBar />
        <div className="project-page">
          <Outlet />
        </div>
      </div>
    </ProjectContext.Provider>
  );
}
