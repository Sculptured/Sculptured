import { useNavigate, useParams } from 'react-router-dom';
import { seatIdFor } from '../draft/ids';
import type { TeamDraft } from '../generated/TeamDraft';
import { agentPagePath, boardPath } from '../paths';
import { useShell } from '../shell/ShellContext';
import { Btn, Icon } from '../shell/ui';
import type { WorkflowTarget } from '../workflow/WorkflowGraph';
import { WorkflowInputEditor, WorkflowInspector } from '../workflow/WorkflowInspector';
import { useProject } from './ProjectContext';

/** `input`, `settings`, `run`, `list` or `stage:<id>` from the address. */
function targetOf(raw: string | undefined): WorkflowTarget | 'list' | null {
  if (raw === 'input' || raw === 'settings' || raw === 'run' || raw === 'list') return raw;
  if (raw?.startsWith('stage:')) return { stage: raw.slice('stage:'.length) };
  return null;
}

/** The workflow's Input as its own page: one roomy column, like an agent's Setup. */
function InputPage({ draft, locked, edit, onBack }: { draft: TeamDraft; locked: boolean; edit: (fn: (d: TeamDraft) => TeamDraft) => void; onBack: () => void }) {
  return (
    <main className="page workflow-page input-page">
      <div className="setup-col input-col">
        <header className="input-page-head">
          <Btn variant="ghost" size="sm" icon="arrow-left" className="input-page-back" onClick={onBack}>Back to the board</Btn>
          <h1 className="input-page-title">
            <span className="input-card-icon" aria-hidden="true"><Icon name="inbox" size={18} /></span>
            Input
          </h1>
          <p className="input-page-lede">Links, notes and files the team starts from. The dispatcher reads everything here when you start the team; nothing runs before then.</p>
        </header>
        <WorkflowInputEditor draft={draft} locked={locked} edit={edit} />
      </div>
    </main>
  );
}

/** `/p/:team/workflow/:target`: one part of a team's workflow, on its own page. */
export function WorkflowPage() {
  const p = useProject();
  const shell = useShell();
  const navigate = useNavigate();
  const target = targetOf(useParams().target);
  const draft = p.draft;
  if (!draft?.workflow || !target)
    return (
      <main className="page page-form workflow-page">
        <p className="muted">{draft ? 'This team has no workflow.' : 'Loading…'}</p>
        <Btn icon="arrow-left" onClick={shell.back}>Back to the board</Btn>
      </main>
    );
  const locked = target === 'run' || (p.live ? p.running : p.t.serverLive);
  const openAgent = (id: string) => { const seat = seatIdFor(draft, id); if (seat) navigate(agentPagePath(p.team, seat)); };
  if (target === 'input') return <InputPage draft={draft} locked={locked} edit={(fn) => p.t.edit(fn)} onBack={() => navigate(boardPath(p.team))} />;
  return (
    <main className="page page-form workflow-page">
      <WorkflowInspector draft={draft} target={target} locked={locked} edit={(fn) => p.t.edit(fn)} onOpen={openAgent} onClose={() => navigate(boardPath(p.team))} closeLabel="Back to the board" className="as-page" />
    </main>
  );
}
