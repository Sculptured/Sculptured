import type { DraftAgent } from '../generated/DraftAgent';
import type { TeamDraft } from '../generated/TeamDraft';
import { agentComponentNames } from './components';
import './components.css';

/** The components an agent uses, as small chips on its card. Components are not agents, so they get no
 * card or arrow of their own. */
export function ComponentChips({ agent, draft }: { agent: DraftAgent; draft?: TeamDraft | null }) {
  const names = agentComponentNames(draft, agent);
  if (names.length === 0) return null;
  return (
    <span className="component-chips" aria-label={`Uses ${names.join(', ')}`}>
      {names.map((n) => <span key={n} className="component-chip" title={`Component: ${n}`}>▣ {n}</span>)}
    </span>
  );
}
