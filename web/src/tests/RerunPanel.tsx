import { useState } from 'react';
import { Btn } from '../shell/ui';
import { changedFields } from './model';
import { RestoreChoice, blockedReason, useRestorePlan, type PlanLoader } from './RestoreChoice';
import { ErrorNote } from './shared';
import { ModelHint, ModelInput, ProviderSelect } from './ProviderFields';
import { forkProblem, providerId, providerOf, someProviderOk, useProviderList } from './providers';
import { KINDS } from './team';
import type { AgentKind, AgentSpec, Change, ProviderStatus, RestoreMode } from './types';

type EditProps = { agent: AgentSpec; edit: Change; openOk: boolean; list: ProviderStatus[]; onEdit: (patch: Change) => void };

/** The mode select; API mode is disabled, saying why, when no provider can take calls. */
function ModeSelect({ kind, own, openOk, onPick }: { kind: AgentKind; own: AgentKind; openOk: boolean; onPick: (k: AgentKind) => void }) {
  return (
    <label className="tst-field">
      <span className="tst-label">Agent</span>
      <select value={kind} onChange={(ev) => onPick(ev.target.value as AgentKind)}>
        {KINDS.map((k) => (
          <option key={k.value} value={k.value} disabled={k.value === 'open' && !openOk && own !== 'open'}>
            {k.label}{k.value === 'open' && !openOk ? ' (no model provider is available)' : ''}
          </option>
        ))}
      </select>
    </label>
  );
}

/** The agent's mode, (API mode) provider and model; a new mode or provider starts from its default model. */
function ModeAndModel({ agent: a, edit: e, openOk, list, onEdit }: EditProps) {
  const kind = (e.kind ?? a.kind) as AgentKind;
  const provider = kind === 'open' ? providerId(e.provider ?? a.provider) : null;
  const own = kind === a.kind && (provider == null || provider === providerOf(a));
  const model = e.model ?? (own ? a.model ?? '' : '');
  // Back to its own mode or provider: drop the model edit.
  const pickKind = (k: AgentKind) => onEdit({ kind: k, model: k === a.kind ? undefined : '', provider: undefined });
  const pickProvider = (id: string) => onEdit({ provider: id, model: kind === a.kind && id === providerOf(a) ? undefined : '' });
  return (
    <>
      <div className="tst-row">
        <ModeSelect kind={kind} own={a.kind} openOk={openOk} onPick={pickKind} />
        {provider && <ProviderSelect value={provider} list={list} onChange={pickProvider} />}
        <ModelInput provider={provider} value={model} list={list} onChange={(m) => onEdit({ model: m })} />
      </div>
      {provider && <ModelHint provider={provider} model={model} list={list} />}
    </>
  );
}

/** One agent's mode, provider, model and instructions, as edited for the fork. */
function AgentEdit({ changed, ...p }: EditProps & { changed: boolean }) {
  return (
    <div className={`tst-rerun-agent${changed ? ' changed' : ''}`}>
      <div className="tst-rerun-agent-head">
        <strong>{p.agent.name}</strong>
        {changed && <span className="tst-changed">changed</span>}
      </div>
      <ModeAndModel {...p} />
      <label className="tst-field">
        <span className="tst-label">Instructions</span>
        <textarea rows={3} value={p.edit.prompt ?? p.agent.prompt} onChange={(ev) => p.onEdit({ prompt: ev.target.value })} />
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
  /** The health check's providers (locals not checked); the panel asks `/api/providers` itself. */
  providers?: ProviderStatus[];
  plan: PlanLoader;
  start: ForkStarter;
  onClose: () => void;
};

/** The panel's state: the edits, the restore mode and its plan, and starting the fork. */
function useFork({ agents, seq, running, plan: load, start }: PanelProps, list: ProviderStatus[]) {
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
  return { edits, edit, changes, restore, setRestore, plan, busy, error, go, blocked: blockedReason(plan, running) ?? forkProblem(agents, changes, list) };
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
  const { list } = useProviderList(props.providers);
  const r = useFork(props, list);
  const openOk = someProviderOk(list, props.openAvailable);
  const n = Object.values(r.changes).reduce((s, c) => s + Object.keys(c).length, 0);
  return (
    <section className="tst-card tst-rerun" aria-label={`Fork from step ${props.seq}`}>
      <RerunHead seq={props.seq} onClose={props.onClose} />
      <div className="tst-rerun-agents">
        {props.agents.map((a) => (
          <AgentEdit key={a.id} agent={a} edit={r.edits[a.id] ?? {}} changed={!!r.changes[a.id]} openOk={openOk} list={list} onEdit={(p) => r.edit(a.id, p)} />
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
