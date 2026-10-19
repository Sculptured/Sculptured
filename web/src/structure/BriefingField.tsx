import { useId } from 'react';
import { setAdvanced } from '../draft/edits';
import type { BriefingLevel } from '../generated/BriefingLevel';
import type { DraftAgent } from '../generated/DraftAgent';
import { Field } from './AgentFields';
import { LOCK_REASON, type Edit } from './Panels';

const BRIEFING_WORDS: Record<BriefingLevel, string> = { full: 'full: the team, its neighbours and the working rules', minimal: 'minimal: only its own job and how to finish' };

/** How much of the team the agent is told about with its first message (full by default). */
export function BriefingField({ a, edit, locked }: { a: DraftAgent; edit: Edit; locked: boolean }) {
  const ids = useId();
  return (
    <Field label="Briefing" htmlFor={`${ids}-briefing`} reason={locked ? LOCK_REASON : null} helpId={`${ids}-briefinghint`}
      help="What the agent is told with its first message. Minimal leaves out the team, other agents' jobs and the working rules, which it would read again on every step; use it for agents that only do their own assignments.">
      <select id={`${ids}-briefing`} aria-describedby={`${ids}-briefinghint`} className="fld-input" value={a.advanced.briefing ?? 'full'} disabled={locked}
        onChange={(e) => edit((d) => setAdvanced(d, a.id, { briefing: e.target.value === 'minimal' ? 'minimal' : undefined }))}>
        {(Object.keys(BRIEFING_WORDS) as BriefingLevel[]).map((b) => <option key={b} value={b}>{BRIEFING_WORDS[b]}</option>)}
      </select>
    </Field>
  );
}
