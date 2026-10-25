import { useId } from 'react';
import { setStatusOwner, statusOwner } from '../draft/statusOwner';
import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';
import { Field } from './AgentFields';
import { LOCK_REASON, type Edit } from './Panels';

/** Whether this agent writes the team's status page (Results → Status); one agent does. */
export function StatusOwnerField({ draft, a, edit, locked }: { draft: TeamDraft; a: DraftAgent; edit: Edit; locked: boolean }) {
  const ids = useId();
  const owner = statusOwner(draft);
  const chosen = !!a.advanced.writes_status;
  const now = owner?.id === a.id ? 'This agent writes it' : `${owner?.name || 'Unnamed agent'} writes it`;
  return (
    <Field label="Status page" htmlFor={`${ids}-status`} reason={locked ? LOCK_REASON : null} helpId={`${ids}-statushint`}
      help="The one page under Results → Status that says where the work stands and what changed, kept current while the team works and finished when the work is done. One agent writes it: the one chosen here, or else the agent at the end of the line.">
      <label className="fld-checkline">
        <input id={`${ids}-status`} type="checkbox" checked={chosen} disabled={locked} aria-describedby={`${ids}-statushint ${ids}-statusnow`}
          onChange={(e) => edit((d) => setStatusOwner(d, e.target.checked ? a.id : null))} />
        This agent writes the team's status page
      </label>
      <p id={`${ids}-statusnow`} className="muted fld-hint">Now: {now}{owner && !owner.advanced.writes_status ? ' (chosen automatically)' : ''}.</p>
    </Field>
  );
}
