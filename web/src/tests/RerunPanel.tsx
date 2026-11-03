import { useState } from 'react';
import { Btn } from '../shell/ui';
import { changedFields } from './model';
import { RestoreChoice, blockedReason, useRestorePlan, type PlanLoader } from './RestoreChoice';
import { ErrorNote } from './shared';
import { DEFAULT_OPENROUTER_MODEL, KINDS } from './team';
import type { AgentKind, AgentSpec, Change, RestoreMode } from './types';

/** The agent's mode and model; a new mode starts from that mode's default model. */
function ModeAndModel({ agent: a, edit: e, openAvailable, onEdit }: { agent: AgentSpec; edit: Change; openAvailable: boolean; onEdit: (patch: Change) => void }) {
  const kind = (e.kind ?? a.kind) as AgentKind;
  // Back to its own mode: drop the model edit.
  const pick = (k: AgentKind) => onEdit({ kind: k, model: k === a.kind ? undefined : '' });
  return (
    <div className="tst-row">
      <label className="tst-field">
        <span className="tst-label">Agent</span>
        <select value={kind} onChange={(ev) => pick(ev.target.value as AgentKind)}>
          {KINDS.map((k) => (
            <option key={k.value} value={k.value} disabled={k.value === 'open' && !openAvailable && a.kind !== 'open'}>
              {k.label}{k.value === 'open' && !openAvailable ? ' (needs OPENROUTER_API_KEY)' : ''}
            </option>
          ))}
        </select>
      </label>
      <label className="tst-field tst-grow">
        <span className="tst-label">Model</span>
        <input className="mono" value={e.model ?? (kind === a.kind ? a.model ?? '' : '')} onChange={(ev) => onEdit({ model: ev.target.value })}
          placeholder={kind === 'open' ? `${DEFAULT_OPENROUTER_MODEL} (default)` : 'default for this agent'} />
      </label>
    </div>
  );
}

/** One agent's mode, model and instructions, as edited for the fork. */
function AgentEdit({ agent: a, edit: e, changed, openAvailable, onEdit }: {
  agent: AgentSpec;
  edit: Change;
  changed: boolean;
  openAvailable: boolean;
  onEdit: (patch: Change) => void;
}) {
  return (
    <div className={`tst-rerun-agent${changed ? ' changed' : ''}`}>
      <div className="tst-rerun-agent-head">
        <strong>{a.name}</strong>
        {changed && <span className="tst-changed">changed</span>}
      </div>
      <ModeAndModel agent={a} edit={e} openAvailable={openAvailable} onEdit={onEdit} />
      <label className="tst-field">
        <span className="tst-label">Instructions</span>
        <textarea rows={3} value={e.prompt ?? a.prompt} onChange={(ev) => onEdit({ prompt: ev.target.value })} />
      </label>
    </div>
  );
}

/** Starts the fork with the changed fields and the restore mode; rejects with why it could not. */
export type ForkStarter = (changes: Record<string, Change>, mode: RestoreMode) => Promise<void>;

type PanelProps = {
  /** The branch's agents as they ran. */
  agents: AgentSpec[];
  seq: number;
  /** The branch is still running (a fork waits for it). */
  running: boolean;
  openAvailable: boolean;
  plan: PlanLoader;
  start: ForkStarter;
  onClose: () => void;
};

/** The panel's state: the edits, the restore mode and its plan, and starting the fork. */
function useFork({ agents, seq, running, plan: load, start }: PanelProps) {
  const [edits, setEdits] = useState<Record<string, Change>>({});
  const [restore, setRestore] = useState<RestoreMode>('exact');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const plan = useRestorePlan(load, seq, restore);
  const changes = changedFields(agents, edits);
  const edit = (id: string, patch: Change) => setEdits((e) => ({ ...e, [id]: { ...e[id], ...patch } }));
  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      await start(changes, restore);
    } catch (e) {
      setError(e);
      setBusy(false);
    }
  };
  return { edits, edit, changes, restore, setRestore, plan, busy, error, go, blocked: blockedReason(plan, running) };
}

function RerunHead({ seq, onClose }: { seq: number; onClose: () => void }) {
  return (
    <header className="tst-rerun-head">
      <div>
        <h2 className="tst-h2">Fork from step {seq}</h2>
        <p className="muted tst-small">
          Step {seq} runs again with your changes, on the world as it was just before it. Steps before it are reused, not run again.
          A Claude Code or Codex agent can't resume mid-way, so a change to one restarts it from its own first step.
        </p>
      </div>
      <Btn size="sm" variant="ghost" icon="x" iconOnly aria-label="Close" onClick={onClose} />
    </header>
  );
}

/** The start button: disabled, saying why, while the branch runs or the restore is refused. */
function ForkFoot({ changes: n, busy, blocked, onGo }: { changes: number; busy: boolean; blocked: string | null; onGo: () => void }) {
  return (
    <footer className="tst-rerun-foot">
      <Btn variant="primary" icon="play" onClick={onGo} disabled={busy || blocked != null}>
        {busy ? 'Forking…' : n ? `Fork with ${n} change${n === 1 ? '' : 's'}` : 'Fork unchanged'}
      </Btn>
      {blocked && <span className="tst-small" role="status">{blocked}</span>}
      {!n && !blocked && <span className="muted tst-small">With no changes, the fork shows how much the result varies on its own.</span>}
    </footer>
  );
}

/**
 * Change one or more agents and fork from a step. The step is done again with the change, on the
 * world as it was just before it, restored exactly or approximately; everything earlier is reused.
 * Only edited fields are sent.
 */
export function RerunPanel(props: PanelProps) {
  const r = useFork(props);
  const n = Object.values(r.changes).reduce((s, c) => s + Object.keys(c).length, 0);
  return (
    <section className="tst-card tst-rerun" aria-label={`Fork from step ${props.seq}`}>
      <RerunHead seq={props.seq} onClose={props.onClose} />
      <div className="tst-rerun-agents">
        {props.agents.map((a) => (
          <AgentEdit key={a.id} agent={a} edit={r.edits[a.id] ?? {}} changed={!!r.changes[a.id]} openAvailable={props.openAvailable} onEdit={(p) => r.edit(a.id, p)} />
        ))}
      </div>
      <div className="tst-rerun-commit">
        <RestoreChoice mode={r.restore} onMode={r.setRestore} plan={r.plan} />
        <ErrorNote error={r.error} />
        <ForkFoot changes={n} busy={r.busy} blocked={r.blocked} onGo={r.go} />
      </div>
    </section>
  );
}
