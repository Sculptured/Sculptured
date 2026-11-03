import { useEffect, useState, type ReactNode } from 'react';
import { Btn } from '../shell/ui';
import { arena } from './api';
import { costSource, diffHunks, dollars, jsonDiff, rowKeys, stepBefore, type Step } from './model';
import { notComparedLine, savedLine } from './restore';
import { ErrorNote, pretty } from './shared';
import { KIND } from './Swimlanes';
import type { BranchDetail, Event } from './types';

const keysOf = (v: unknown): string[] => (Array.isArray(v) ? v.filter((k): k is string => typeof k === 'string') : []);

function Section({ title, children, open = true }: { title: string; children: ReactNode; open?: boolean }) {
  return (
    <details className="tst-sec" open={open}>
      <summary>{title}</summary>
      <div className="tst-sec-body">{children}</div>
    </details>
  );
}

const Json = ({ value }: { value: unknown }) => <pre className="tst-json">{pretty(value)}</pre>;

type BeforeAfter = { before: Record<string, unknown>; after: Record<string, unknown> };

/** The rows `keys` just before step `seq` and just after it. */
function useBeforeAfter(branch: BranchDetail, seq: number, keys: string[]) {
  const [rows, setRows] = useState<BeforeAfter | null>(null);
  const [error, setError] = useState<unknown>(null);
  const joined = keys.join(',');
  useEffect(() => {
    let alive = true;
    const list = joined.split(',');
    setRows(null); setError(null);
    const prev = stepBefore(branch.events, seq);
    Promise.all([prev == null ? Promise.resolve({ rows: {} }) : arena.rows(branch.id, prev, list), arena.rows(branch.id, seq, list)])
      .then(([b, a]) => { if (alive) setRows({ before: b.rows, after: a.rows }); })
      .catch((e) => { if (alive) setError(e); });
    return () => { alive = false; };
  }, [branch.id, branch.events, seq, joined]);
  return { rows, error };
}

function DiffLines({ a, b }: { a: unknown; b: unknown }) {
  const hunks = diffHunks(jsonDiff(a ?? undefined, b ?? undefined));
  if (hunks.length === 0) return <p className="muted tst-small">No change.</p>;
  return (
    <pre className="tst-json tst-diff">
      {hunks.map((l, i) => l == null
        ? <div key={i} className="dl-gap">⋯</div>
        : <div key={i} className={l.op === '+' ? 'dl-add' : l.op === '-' ? 'dl-del' : 'dl-same'}>{l.op} {l.text}</div>)}
    </pre>
  );
}

/** Each row this write changed, as it was before the step and just after. */
function RowChanges({ branch, seq, keys }: { branch: BranchDetail; seq: number; keys: string[] }) {
  const { rows, error } = useBeforeAfter(branch, seq, keys);
  if (error) return <ErrorNote error={error} />;
  if (!rows) return <p className="muted tst-small">Reading the rows before and after this step…</p>;
  return (
    <>
      {keys.map((k) => (
        <div key={k} className="tst-rowdiff">
          <div className="mono tst-small">{k}{rows.before[k] == null && <span className="muted"> (new)</span>}</div>
          <DiffLines a={rows.before[k]} b={rows.after[k]} />
        </div>
      ))}
    </>
  );
}

/** Rows a step read, as they were just after it, on request (each read restores a copy of the save). */
function RowsRead({ branch, seq, keys, more }: { branch: BranchDetail; seq: number; keys: string[]; more: number }) {
  const [rows, setRows] = useState<Record<string, unknown> | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setRows(null); setError(null); }, [branch.id, seq]);
  if (keys.length === 0) return <p className="muted tst-small">This step read no particular rows.</p>;
  return (
    <>
      <div className="tst-keys">{keys.map((k) => <span key={k} className="chip mono">{k}</span>)}{more > 0 && <span className="muted tst-small">+{more} more</span>}</div>
      {!rows && <Btn size="sm" onClick={() => {
        setBusy(true);
        arena.rows(branch.id, seq, keys).then((r) => setRows(r.rows)).catch(setError).finally(() => setBusy(false));
      }} disabled={busy}>{busy ? 'Reading…' : 'Show these rows'}</Btn>}
      <ErrorNote error={error} />
      {rows && Object.entries(rows).map(([k, doc]) => (
        <div key={k} className="tst-rowdiff">
          <div className="mono tst-small">{k}{doc === null && <span className="muted"> (absent)</span>}</div>
          {doc !== null && <Json value={doc} />}
        </div>
      ))}
    </>
  );
}

function messageText(m: unknown): string {
  const c = (m as { content?: unknown })?.content;
  return typeof c === 'string' ? c : pretty(c ?? m);
}

/** What the inspector reads from a step's payload (each kind has some of these). */
type P = {
  error?: unknown; snapshot_error?: unknown; writes?: unknown; reads?: unknown; args?: unknown; result?: unknown;
  saves?: Record<string, string>; snapshot?: string | null;
  request?: { messages?: unknown }; response?: { choices?: { message?: unknown }[] };
  cost_usd?: number | null; cost_source?: string | null; note?: unknown;
  passed?: boolean; differing_rows?: unknown; unchecked?: string[] | null;
  ok?: boolean; seconds?: number | null; tokens?: number | null;
};

/** After a write: what was saved, so a fork can start there, or why nothing could be. */
function SavedNote({ p }: { p: P }) {
  if (savedLine(p)) return <p className="muted tst-small">{savedLine(p)}, so a fork can start from here.</p>;
  if (!p.snapshot_error) return null;
  return <div className="tst-note tst-note-error"><span>The world could not be saved after this step, so a fork can't start right after it.</span><span className="tst-evidence mono" data-raw-evidence>{String(p.snapshot_error)}</span></div>;
}

function ToolCall({ branch, event, p }: { branch: BranchDetail; event: Event; p: P }) {
  const writes = p.error == null ? keysOf(p.writes) : [];
  const { keys, more } = rowKeys([], keysOf(p.reads).filter((k) => !writes.includes(k)));
  return (
    <>
      {p.error != null && <div className="tst-note tst-note-error"><span>The tool refused this call.</span><span className="tst-evidence mono" data-raw-evidence>{String(p.error)}</span></div>}
      {writes.length > 0 && (
        <Section title={`Rows changed (${writes.length})`}>
          <RowChanges branch={branch} seq={event.seq} keys={writes.slice(0, 50)} />
          <SavedNote p={p} />
        </Section>
      )}
      <Section title="Arguments"><Json value={p.args} /></Section>
      {p.error == null && <Section title="Result" open={writes.length === 0}><Json value={p.result} /></Section>}
      <Section title={`Rows read (${keys.length + more})`} open={false}><RowsRead branch={branch} seq={event.seq} keys={keys} more={more} /></Section>
    </>
  );
}

function ModelCall({ p }: { p: P }) {
  const msgs: unknown[] = Array.isArray(p.request?.messages) ? p.request.messages : [];
  return (
    <>
      <Section title="Response"><Json value={p.response?.choices?.[0]?.message ?? p.response} /></Section>
      <Section title={`Conversation so far (${msgs.length} messages)`} open={false}>
        {msgs.map((m, i) => (
          <div key={i} className="tst-msg">
            <span className="tst-label">{(m as { role?: string })?.role ?? 'message'}</span>
            <pre className="tst-json">{messageText(m)}</pre>
          </div>
        ))}
      </Section>
      <p className="muted tst-small">{p.cost_usd != null ? `${dollars(p.cost_usd)} (${costSource(p.cost_source)})` : 'No dollar figure for this call.'}</p>
    </>
  );
}

function Check({ p }: { p: P }) {
  return (
    <>
      <p>{p.passed ? '✓ The final world matches what the task expects.' : '✕ The final world does not match what the task expects.'}</p>
      {Array.isArray(p.differing_rows) && p.differing_rows.length > 0 && <Section title={`Rows that differ (${p.differing_rows.length})`}><Json value={p.differing_rows} /></Section>}
      {notComparedLine(p.unchecked, 'check') && <p className="tst-small">{notComparedLine(p.unchecked, 'check')}</p>}
    </>
  );
}

function AgentEnd({ p }: { p: P }) {
  return (
    <>
      <p>{p.ok ? 'Finished.' : 'Stopped with an error.'}</p>
      {p.error != null && <div className="tst-note tst-note-error"><span>What the agent's program reported:</span><span className="tst-evidence mono" data-raw-evidence>{String(p.error)}</span></div>}
      <dl className="tst-kv">
        <dt>Time</dt><dd>{p.seconds != null ? `${Number(p.seconds).toFixed(1)} s` : 'unknown'}</dd>
        <dt>Tokens</dt><dd>{p.tokens != null ? Number(p.tokens).toLocaleString() : 'unknown'}</dd>
        <dt>Cost</dt><dd>{p.cost_usd != null ? `${dollars(p.cost_usd)} · ${costSource(p.cost_source)}` : 'tokens only (no dollar figure)'}</dd>
      </dl>
    </>
  );
}

function Body({ branch, event }: { branch: BranchDetail; event: Event }) {
  const p = (event.payload ?? {}) as P;
  switch (event.kind) {
    case 'tool_call': return <ToolCall branch={branch} event={event} p={p} />;
    case 'model_call': return <ModelCall p={p} />;
    case 'handoff': return <Section title="What it handed on"><blockquote className="tst-quote">{String(p.note ?? '')}</blockquote></Section>;
    case 'check': return <Check p={p} />;
    case 'agent_end': return <AgentEnd p={p} />;
    case 'agent_start': {
      const a = branch.team.agents.find((x) => x.id === event.agent);
      return a ? <Section title="Its instructions"><pre className="tst-json tst-prose">{a.prompt || '(the default for this role)'}</pre></Section> : null;
    }
    default: return <Json value={p} />;
  }
}

/** The selected step in full: what it did, the rows it changed and read. */
export function StepInspector({ branch, event, step }: { branch: BranchDetail; event: Event | null; step: Step | null }) {
  if (!event || !step) {
    return (
      <div className="tst-inspector tst-inspector-empty">
        <p><strong>Pick a step</strong></p>
        <p className="muted">Click a step in the lanes, or use ← and → to move between steps.</p>
      </div>
    );
  }
  const agent = branch.team.agents.find((a) => a.id === event.agent);
  const k = KIND[step.kind];
  return (
    <div className="tst-inspector">
      <header className="tst-insp-head">
        <span className={`tst-glyph-tile k-${step.kind}`} aria-hidden="true">{step.failed ? '✕' : k.glyph}</span>
        <div>
          <div className="tst-label">Step {event.seq} · {agent?.name ?? 'Check'}</div>
          <div className="tst-insp-title">{event.kind === 'tool_call' ? <span className="mono">{step.label}</span> : step.label}</div>
          <div className="muted tst-small">{k.word !== step.label ? `${k.word} · ` : ''}at {(step.to / 1000).toFixed(1)} s{step.to > step.from ? ` · took ${((step.to - step.from) / 1000).toFixed(1)} s` : ''}{step.inherited ? ' · reused from the original' : ''}</div>
        </div>
      </header>
      <div className="tst-insp-body"><Body branch={branch} event={event} /></div>
    </div>
  );
}
