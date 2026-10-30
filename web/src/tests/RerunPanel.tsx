import { useState } from 'react';
import { Btn } from '../shell/ui';
import { arena } from './api';
import { changedFields } from './model';
import { ErrorNote } from './shared';
import { DEFAULT_OPENROUTER_MODEL, KINDS } from './team';
import type { AgentKind, BranchDetail, Change, ForkResponse } from './types';

/**
 * Change one or more agents and rerun from a step. The step is done again with the change, on the
 * database as it was just before it; everything earlier is reused. Only edited fields are sent.
 */
export function RerunPanel({ branch, seq, openAvailable, onDone, onClose }: {
  branch: BranchDetail;
  seq: number;
  openAvailable: boolean;
  onDone: (r: ForkResponse) => void;
  onClose: () => void;
}) {
  const [edits, setEdits] = useState<Record<string, Change>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const set = (id: string, patch: Change) => setEdits((e) => ({ ...e, [id]: { ...e[id], ...patch } }));
  const changes = changedFields(branch.team.agents, edits);
  const n = Object.values(changes).reduce((s, c) => s + Object.keys(c).length, 0);
  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      onDone(await arena.fork(branch.id, seq, changes));
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  return (
    <section className="tst-card tst-rerun" aria-label={`Change and rerun from step ${seq}`}>
      <header className="tst-rerun-head">
        <div>
          <h2 className="tst-h2">Change &amp; rerun from step {seq}</h2>
          <p className="muted tst-small">
            Step {seq} runs again with your changes, on the database as it was just before it. Steps before it are reused, not rerun.
            A Claude Code or Codex agent can't resume mid-way, so a change to one restarts it from its own first step.
          </p>
        </div>
        <Btn size="sm" variant="ghost" icon="x" iconOnly aria-label="Close" onClick={onClose} />
      </header>
      <div className="tst-rerun-agents">
        {branch.team.agents.map((a) => {
          const e = edits[a.id] ?? {};
          const kind = (e.kind ?? a.kind) as AgentKind;
          return (
            <div key={a.id} className={`tst-rerun-agent${changes[a.id] ? ' changed' : ''}`}>
              <div className="tst-rerun-agent-head">
                <strong>{a.name}</strong>
                {changes[a.id] && <span className="tst-changed">changed</span>}
              </div>
              <div className="tst-row">
                <label className="tst-field">
                  <span className="tst-label">Agent</span>
                  <select value={kind} onChange={(ev) => {
                    const k = ev.target.value as AgentKind;
                    // Back to its own mode: drop the model edit; a new mode starts from that mode's default model.
                    set(a.id, { kind: k, model: k === a.kind ? undefined : '' });
                  }}>
                    {KINDS.map((k) => (
                      <option key={k.value} value={k.value} disabled={k.value === 'open' && !openAvailable && a.kind !== 'open'}>
                        {k.label}{k.value === 'open' && !openAvailable ? ' (needs OPENROUTER_API_KEY)' : ''}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="tst-field tst-grow">
                  <span className="tst-label">Model</span>
                  <input
                    className="mono"
                    value={e.model ?? (kind === a.kind ? a.model ?? '' : '')}
                    placeholder={kind === 'open' ? `${DEFAULT_OPENROUTER_MODEL} (default)` : 'default for this agent'}
                    onChange={(ev) => set(a.id, { model: ev.target.value })}
                  />
                </label>
              </div>
              <label className="tst-field">
                <span className="tst-label">Instructions</span>
                <textarea rows={5} value={e.prompt ?? a.prompt} onChange={(ev) => set(a.id, { prompt: ev.target.value })} />
              </label>
            </div>
          );
        })}
      </div>
      <ErrorNote error={error} />
      <footer className="tst-rerun-foot">
        <Btn variant="primary" icon="play" onClick={go} disabled={busy || branch.status === 'running'}>
          {busy ? 'Starting…' : n ? `Rerun with ${n} change${n === 1 ? '' : 's'}` : 'Rerun unchanged'}
        </Btn>
        {branch.status === 'running' && <span className="muted tst-small">Wait for this branch to finish first.</span>}
        {!n && branch.status !== 'running' && <span className="muted tst-small">With no changes, the rerun shows how much the result varies on its own.</span>}
      </footer>
    </section>
  );
}
