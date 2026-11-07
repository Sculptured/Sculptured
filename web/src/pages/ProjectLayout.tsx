import { useEffect, useMemo, useState } from 'react';
import { Outlet, useParams } from 'react-router-dom';
import { liveIssues } from '../draft/issues';
import { useShell } from '../shell/ShellContext';
import { hasProcess } from '../status';
import type { RigState } from '../useRigStream';
import { useTeamDraft } from '../useTeamDraft';
import { ProjectBar } from './ProjectBar';
import { ProjectContext, type Project } from './ProjectContext';

/** `/p/:team`: the project tab row, then the page. A new project starts afresh (its own design and undo). */
export function ProjectLayout() {
  const { team } = useParams();
  return <ProjectScope key={team} team={team!} />;
}

type Shell = ReturnType<typeof useShell>;
type Draft = ReturnType<typeof useTeamDraft>;

const builtHereOf = (shell: Shell, team: string, t: Draft) => !!t.draft || !!shell.summaries?.find((s) => s.id === team) || (!!shell.drafts[team] && !t.loadError);
const startedOf = (shell: Shell, team: string, t: Draft, live: unknown) =>
  !!live || !!shell.summaries?.find((s) => s.id === team)?.started || !!shell.rigs?.some((r) => r.name === team) || t.startedRevision !== null;
const runningOf = (live: RigState | null, t: Draft) => (live ? Object.values(live.seats).some((s) => hasProcess(s.lifecycle)) : t.serverLive);

/** The project as the pages see it: its design, live state and whether it can run. */
function useProjectValue(team: string, t: Draft, startBlock: string | null, setStartBlock: (id: string | null) => void): Project {
  const shell = useShell();
  const stream = shell.teams[team];
  const live = stream?.state.snapshot ? stream.state : null;
  const row = shell.rows.find((r) => r.rig === team);
  const issues = useMemo(() => (t.draft ? liveIssues(t.draft, t.issues) : []), [t.draft, t.issues]);
  return {
    team,
    title: t.draft?.name.trim() || shell.titles[team] || row?.title || team,
    t,
    draft: t.draft,
    issues,
    builtHere: builtHereOf(shell, team, t),
    started: startedOf(shell, team, t, live),
    live,
    connected: !!stream?.connected,
    running: runningOf(live, t),
    row,
    startBlock,
    setStartBlock,
  };
}

function ProjectScope({ team }: { team: string }) {
  const shell = useShell();
  const t = useTeamDraft(team);
  const { onDraft } = shell;
  useEffect(() => { if (t.draft) onDraft(t.draft); }, [t.draft, onDraft]);
  const [startBlock, setStartBlock] = useState<string | null>(null);
  const project = useProjectValue(team, t, startBlock, setStartBlock);
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
