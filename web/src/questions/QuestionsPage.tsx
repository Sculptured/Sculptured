import { useState } from 'react';
import { api } from '../api';
import { seatIdFor } from '../draft/ids';
import type { EventRecord } from '../generated/EventRecord';
import type { QuestionView } from '../generated/QuestionView';
import type { TeamDraft } from '../generated/TeamDraft';
import { useProject } from '../pages/ProjectContext';
import { useShell } from '../shell/ShellContext';
import { usePolled } from '../usePolling';
import { QuestionLog } from './QuestionLog';

/** Question events in a feed: a change means the list is re-read at once (it is also polled slowly). */
const questionEvents = (feed: EventRecord[]) => feed.filter((e) => e.body.type.startsWith('question')).length;

/** An agent's Questions tab: everything it asked you, open ones answered here. */
export function AgentQuestions({ team, seat, name, feed }: { team: string; seat: string; name: string; feed: EventRecord[] }) {
  const { data, error, reload } = usePolled(() => api.seatQuestions(seat), 15_000, [seat, questionEvents(feed)]);
  return (
    <section className="ap-body questions-tab" aria-label="Questions">
      {error && <p className="tone-red" role="alert">Could not load the questions: {error}</p>}
      {data === null ? !error && <p className="muted" role="status">Loading questions…</p> : <QuestionLog team={team} questions={data} names={{ [seat]: name }} showAgent={false} onChanged={reload} />}
    </section>
  );
}

/** Each asking agent's name: the design's, else the started team's, else its id. */
function agentNames(draft: TeamDraft | null, questions: QuestionView[], live: (seat: string) => string | undefined): Record<string, string> {
  const names: Record<string, string> = {};
  for (const a of draft?.agents ?? []) {
    const seat = seatIdFor(draft!, a.id);
    if (seat) names[seat] = a.name.trim() || a.id;
  }
  for (const q of questions) names[q.seat] ??= live(q.seat) ?? q.seat.split('@')[0];
  return names;
}

/**
 * `/p/:team/questions`: every question the team's agents asked you, in one place: what is waiting, your
 * answers and the choices they picked from, what agents decided when nobody answered, and withdrawn ones.
 */
export function QuestionsPage() {
  const p = useProject();
  const shell = useShell();
  const feed = Object.values(p.live?.feed ?? {}).flat();
  const { data, error, reload } = usePolled(() => api.teamQuestions(p.team), 15_000, [p.team, questionEvents(feed)]);
  const [agent, setAgent] = useState('');
  const names = agentNames(p.draft, data ?? [], (seat) => shell.byId[seat]?.name);
  const asking = [...new Set((data ?? []).map((q) => q.seat))];
  const shown = (data ?? []).filter((q) => !agent || q.seat === agent);
  return (
    <main className="page page-wide questions-page">
      <header className="questions-head">
        <h1>Questions</h1>
        <p className="muted">What the agents asked you, your answers and the choices they offered, and what they decided when nobody answered.</p>
        {asking.length > 1 && (
          <label className="fld"><span>Agent</span>
            <select className="fld-input" value={agent} onChange={(e) => setAgent(e.target.value)}>
              <option value="">Every agent</option>
              {asking.map((s) => <option key={s} value={s}>{names[s]}</option>)}
            </select></label>
        )}
      </header>
      {error && <p className="tone-red" role="alert">Could not load the questions: {error}</p>}
      {data === null ? !error && <p className="muted" role="status">Loading questions…</p> : <QuestionLog team={p.team} questions={shown} names={names} showAgent onChanged={reload} />}
    </main>
  );
}
