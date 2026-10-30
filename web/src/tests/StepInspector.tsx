import { useEffect, useState, type ReactNode } from 'react';
import { Btn } from '../shell/ui';
import { arena } from './api';
import { costSource, diffHunks, dollars, jsonDiff, rowKeys, stepBefore, type Step } from './model';
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

/** Each row this write changed, as it was before the step and just after. */
function RowChanges({ branch, seq, keys }: { branch: BranchDetail; seq: number; keys: string[] }) {
  const [rows, setRows] = useState<{ before: Record<string, unknown>; after: Record<string, unknown> } | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let alive = true;
    setRows(null); setError(null);
    const prev = stepBefore(branch.events, seq);
    Promise.all([prev == null ? Promise.resolve({ rows: {} }) : arena.rows(branch.id, prev, keys), arena.rows(branch.id, seq, keys)])
      .then(([b, a]) => { if (alive) setRows({ before: b.rows, after: a.rows }); })
      .catch((e) => { if (alive) setError(e); });
    return () => { alive = false; };
  }, [branch.id, seq, keys.join(',')]); // eslint-disable-line react-hooks/exhaustive-deps
  if (error) return <ErrorNote error={error} />;
  if (!rows) return <p className="muted tst-small">Reading the rows before and after this step…</p>;
  return (
    <>
      {keys.map((k) => {
        const hunks = diffHunks(jsonDiff(rows.before[k] ?? undefined, rows.after[k] ?? undefined));
        return (
          <div key={k} className="tst-rowdiff">
            <div className="mono tst-small">{k}{rows.before[k] == null && <span className="muted"> (new)</span>}</div>
            {hunks.length === 0 ? <p className="muted tst-small">No change.</p> : (
              <pre className="tst-json tst-diff">
                {hunks.map((l, i) => l == null
                  ? <div key={i} className="dl-gap">⋯</div>
                  : <div key={i} className={l.op === '+' ? 'dl-add' : l.op === '-' ? 'dl-del' : 'dl-same'}>{l.op} {l.text}</div>)}
              </pre>
            )}
          </div>
        );
      })}
    </>
  );
}

/** Rows a step read, as they were just after it, on request (each read restores a copy of the database). */
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

function Body({ branch, event }: { branch: BranchDetail; event: Event }) {
  const p = (event.payload ?? {}) as Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
  switch (event.kind) {
    case 'tool_call': {
      const writes = p.error == null ? keysOf(p.writes) : [];
      const reads = keysOf(p.reads).filter((k) => !writes.includes(k));
      const { keys, more } = rowKeys([], reads);
      return (
        <>
          {p.error != null && <div className="tst-note tst-note-error"><span>The tool refused this call.</span><span className="tst-evidence mono" data-raw-evidence>{String(p.error)}</span></div>}
          {writes.length > 0 && (
            <Section title={`Rows changed (${writes.length})`}>
              <RowChanges branch={branch} seq={event.seq} keys={writes.slice(0, 50)} />
              {p.snapshot ? <p className="muted tst-small">The database was saved after this step, so a rerun can start from here.</p>
                : p.snapshot_error ? <div className="tst-note tst-note-error"><span>The database could not be saved after this step, so a rerun can't start right after it.</span><span className="tst-evidence mono" data-raw-evidence>{String(p.snapshot_error)}</span></div>
                : null}
            </Section>
          )}
          <Section title="Arguments"><Json value={p.args} /></Section>
          {p.error == null && <Section title="Result" open={writes.length === 0}><Json value={p.result} /></Section>}
          <Section title={`Rows read (${keys.length + more})`} open={false}><RowsRead branch={branch} seq={event.seq} keys={keys} more={more} /></Section>
        </>
      );
    }
    case 'model_call': {
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
    case 'handoff':
      return <Section title="What it handed on"><blockquote className="tst-quote">{String(p.note ?? '')}</blockquote></Section>;
    case 'check':
      return (
        <>
          <p>{p.passed ? '✓ The final database matches what the task expects.' : '✕ The final database does not match what the task expects.'}</p>
          {Array.isArray(p.differing_rows) && p.differing_rows.length > 0 && <Section title={`Rows that differ (${p.differing_rows.length})`}><Json value={p.differing_rows} /></Section>}
        </>
      );
    case 'agent_end':
      return (
        <>
          <p>{p.ok ? 'Finished.' : 'Stopped with an error.'}</p>
          {p.error && <div className="tst-note tst-note-error"><span>What the agent's program reported:</span><span className="tst-evidence mono" data-raw-evidence>{String(p.error)}</span></div>}
          <dl className="tst-kv">
            <dt>Time</dt><dd>{p.seconds != null ? `${Number(p.seconds).toFixed(1)} s` : 'unknown'}</dd>
            <dt>Tokens</dt><dd>{p.tokens != null ? Number(p.tokens).toLocaleString() : 'unknown'}</dd>
            <dt>Cost</dt><dd>{p.cost_usd != null ? `${dollars(p.cost_usd)} · ${costSource(p.cost_source)}` : 'tokens only (no dollar figure)'}</dd>
          </dl>
        </>
      );
    case 'agent_start': {
      const a = branch.team.agents.find((x) => x.id === event.agent);
      return a ? <Section title="Its instructions"><pre className="tst-json tst-prose">{a.prompt || '(the default for this role)'}</pre></Section> : null;
    }
    default:
      return <Json value={p} />;
  }
}

/** The selected step in full: what it did, the rows it changed, and Rerun from here. */
export function StepInspector({ branch, event, step, onRerun, canRerun }: {
  branch: BranchDetail;
  event: Event | null;
  step: Step | null;
  onRerun: () => void;
  canRerun: string | null;
}) {
  if (!event || !step) {
    return (
      <div className="tst-inspector tst-inspector-empty">
        <p><strong>Pick a step</strong></p>
        <p className="muted">Click a block in the lanes, or use ← and → to move between steps.</p>
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
      <footer className="tst-insp-foot">
        <Btn variant="primary" icon="refresh" onClick={onRerun} disabled={canRerun != null}>Change &amp; rerun from here</Btn>
        {canRerun && <span className="muted tst-small">{canRerun}</span>}
      </footer>
    </div>
  );
}
