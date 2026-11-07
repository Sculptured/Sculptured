import { useEffect, useRef, useState } from 'react';
import { closeOnEscape } from '../closeOnEscape';
import { api } from '../api';
import { errorText } from '../errors';
import type { DecisionEntry } from '../generated/DecisionEntry';
import { isLearned, markSeen, type Knowledge, type MapArea } from '../knowledge';
import { usePolled } from '../usePolling';
import { Btn } from '../shell/ui';
import './knowledge.css';

const when = (ms: number) => new Date(ms).toLocaleString();

function Entry({ e, onConfirm, onHide }: { e: DecisionEntry; onConfirm: () => void; onHide: () => void }) {
  const lesson = isLearned(e);
  const kind = e.source === 'constraint' ? 'Constraint' : 'Lesson';
  const by = e.source === 'claim-check' ? 'Claim check by Sculptured (heuristic: what the agent said against the tool calls captured)' : lesson ? `${kind} from ${e.author}` : `Decision by ${e.author}`;
  return (
    <li className="kn-entry">
      <p>{e.text}</p>
      <p className="muted">
        {by} · {when(e.at_ms)}
        {e.confirmed_ms ? ' · confirmed by you, kept with the project' : lesson ? ' · reported, not yet confirmed' : ''}
      </p>
      <div className="kn-actions">
        {lesson && !e.confirmed_ms && <Btn size="sm" onClick={onConfirm} aria-label={`Confirm lesson: ${e.text}`}>Confirm</Btn>}
        <Btn size="sm" variant="ghost" onClick={onHide} aria-label={`Hide: ${e.text}`} tip="Hidden entries are no longer told to agents">Hide</Btn>
      </div>
    </li>
  );
}

function Area({ a, who }: { a: MapArea; who?: string[] }) {
  return (
    <li className="kn-area">
      <details>
        <summary>
          <b>{a.path || '(top level)'}</b> <span className="muted">{a.files} files · {a.kinds.map(([k]) => k).join(', ')}</span>
          {a.about && <span> · {a.about}</span>}
          {who?.length ? <span className="muted"> · worked on by {who.join(', ')}</span> : null}
        </summary>
        {a.docs.length === 0 ? <p className="muted">No notes in this area.</p> : (
          <ul>{a.docs.map((d) => <li key={d.file}><code>{d.file}</code>
            {d.sections.length > 0 && <ul>{d.sections.map((s) => <li key={s.line}>{s.title} · lines {s.line}–{s.end}</li>)}</ul>}
          </li>)}</ul>
        )}
      </details>
    </li>
  );
}

function Lessons({ k, rig, act }: { k: Knowledge; rig: string; act: (p: Promise<unknown>) => void }) {
  return (
    <section aria-label="Lessons and decisions">
      <h3>Lessons and decisions</h3>
      <p className="muted">Agents record lessons when something cost them a wrong turn; every agent is told the visible ones when it starts a conversation. Constraints are rules every agent follows. Confirming one marks it as established and keeps it with the project (<code>.sculptured/lessons.md</code>), so later teams there are told it too.</p>
      {k.entries.length === 0 ? <p className="muted">Nothing recorded yet.</p> : (
        <ul className="kn-entries">{k.entries.map((e) => <Entry key={e.id} e={e} onConfirm={() => act(api.confirmDecision(rig, e.id))} onHide={() => act(api.hideDecision(rig, e.id))} />)}</ul>
      )}
    </section>
  );
}

function Maps({ k }: { k: Knowledge }) {
  return (
    <section aria-label="Project map">
      <h3>Project map</h3>
      <p className="muted">Made from the agents’ folders at each start, with nothing to maintain. Agents get its outline and read the rest from <code>.sculptured/map.md</code> in their folder.</p>
      {k.maps.length === 0 ? <p className="muted">No map yet: one is made when the team next starts (the agents’ computer needs a current runner).</p> : k.maps.map((m) => (
        <div key={m.folder} className="kn-map">
          <p><code>{m.folder}</code> · {m.map.files} files{m.map.truncated ? ' (the first of many)' : ''} · made {when(m.at_ms)}{m.map.kept?.length ? ` · ${m.map.kept.length} kept with the project` : ''}</p>
          {!!m.map.shared?.length && <ul className="kn-shared" aria-label="Shared values, checked at the last start">{m.map.shared.map((c) => (
            <li key={`${c.name}-${c.file}`}>{c.status === 'found' ? '✓' : '✗'} <code>{c.name} = {c.value}</code> in <code>{c.file}</code>{c.status === 'found' ? ` line ${c.line} (found)` : c.status === 'missing' ? ' — not in the file' : ' — no such file'}</li>
          ))}</ul>}
          <ul className="kn-areas">{m.map.areas.map((a) => <Area key={a.path} a={a} who={m.workers[a.path]} />)}</ul>
        </div>
      ))}
    </section>
  );
}

/**
 * What the team's agents know: the lessons they recorded and the decisions you made, and the project
 * map Sculptured generated from their folders at the last start. Opened at leisure; never interrupts.
 */
export function KnowledgeView({ rig, onClose }: { rig: string; onClose: () => void }) {
  const [tick, setTick] = useState(0);
  const { data, error } = usePolled(() => api.knowledge(rig), 60000, [rig, tick]);
  const [failed, setFailed] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus(); markSeen(rig); }, [rig]);
  const act = (p: Promise<unknown>) => { p.then(() => setTick((t) => t + 1)).catch((e) => setFailed(errorText(e))); };
  const k: Knowledge | null = data ?? null;
  return (
    <section className="knowledge" aria-label="Knowledge" onKeyDown={closeOnEscape(onClose)}>
      <header><h2 ref={heading} tabIndex={-1}>Knowledge</h2><Btn onClick={onClose}>Back to board</Btn></header>
      {(error || failed) && <p role="alert">{failed ?? error}</p>}
      {!k && !error && <p className="muted">Loading…</p>}
      {k && <><Lessons k={k} rig={rig} act={act} /><Maps k={k} /></>}
    </section>
  );
}
