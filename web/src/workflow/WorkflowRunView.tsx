import {DismissibleError} from '../shell/DismissibleError';
import { useState } from 'react';
import { usePolled } from '../usePolling';
import { Link } from 'react-router-dom';
import { api } from '../api';
import { errorText } from '../errors';
import type { WorkflowRun } from '../generated/WorkflowRun';
import type { WorkflowTask } from '../generated/WorkflowTask';
import type { TeamDraft } from '../generated/TeamDraft';
import { agentPath } from '../shell/model';
import { seatIdFor } from '../draft/ids';
import { TopicExecution } from './TopicExecution';
import { DecisionCard, provenance } from './DecisionCard';
import { ItemBoard, costText, costTitle } from './ItemBoard';
import { DecisionLog } from './DecisionLog';
import { DispatchNotice } from './DispatchCommand';
import type { CostSource } from '../generated/CostSource';
import { usesPipeline } from './itemBoardModel';
import type { TurnTokens } from '../generated/TurnTokens';
import type { WorkflowStep } from '../generated/WorkflowStep';

export function WorkflowRunView({ draft }: { draft: TeamDraft }) {
  const teamId = draft.id;
  const { data, error, reload } = usePolled(() => api.workflow(teamId), 2500, [teamId]);
  if (error) return <DismissibleError>Could not load workflow: {error}</DismissibleError>;
  const notice = <DispatchNotice team={teamId} start={data?.start_seq ?? null} record={data?.dispatch} running={!!data?.dispatching} active={data?.start_seq != null} onRetried={reload} />;
  if (!data?.run) return <>{notice}<p className="muted">No batch dispatched yet. Input and stage design are configuration, not completed work.</p></>;
  const r = data.run, accepted = r.tasks.filter(t => t.status === 'complete').length;
  const waiting = r.tasks.filter(t => t.status === 'waiting');
  return <section className="workflow-run" aria-label="Dispatched work"><h2>{r.batch_id} · {accepted}/{r.tasks.length} items accepted</h2>
    <p>{data.active ? 'Current batch' : 'Previous or stopped batch · no automatic dispatch'} · {assemblyStatus(r, accepted, draft)}</p>
    <progress value={accepted} max={r.tasks.length} aria-label="Accepted interactions" />
    {notice}
    {r.guard && !r.guard.passed && <GuardNotice run={r} active={data.active} onResumed={reload} />}
    {data.active && waiting.length > 0 && <section className="decision-queue" aria-label="Waiting for your decision"><h3>Waiting for you ({waiting.length})</h3>
      {waiting.map(t => <DecisionCard key={t.input.id} t={t} run={r} onDecided={reload} logOn={!!draft.decision_log} />)}</section>}
    {draft.decision_log && <DecisionLog team={teamId} />}
    {usesPipeline(r.config) && <ItemBoard run={r} usage={data.usage ?? {}} />}
    {[...r.tasks, r.assembly].map(t => <Task key={t.input.id} t={t} run={r} draft={draft} active={data.active} usage={data.usage ?? {}} />)}
  </section>;
}

function assemblyStatus(run: WorkflowRun, accepted: number, draft: TeamDraft): string {
  if (run.assembly.status === 'complete') return 'Assembly reported complete';
  if (run.assembly.status === 'blocked') return 'Assembly blocked · inspect its evidence below';
  if (run.assembly.status === 'assigned') return `Assembly assigned to ${draft.agents.find(a => a.id === run.config.collector)?.name || run.config.collector}`;
  return accepted === run.tasks.length ? 'All items accepted · assembly queued' : 'Assembly waits for every item';
}

function Task({ t, run, active, draft, usage }: { t: WorkflowTask; run: WorkflowRun; active: boolean; draft: TeamDraft; usage: Record<string, TurnTokens> }) {
  const [error, setError] = useState('');
  const agent = t.assigned_agent ?? (t.stage === 'assembly' ? run.config.collector : run.config.stages.find(s => s.id === t.stage)?.agent);
  const retry = async () => { try { await api.retryWorkflow(run.rig, t.input.id, run.start_seq); setError(''); } catch (e) { setError(errorText(e)); } };
  return <details id={`item-${t.input.id}`} className={`workflow-task state-${t.status}`}><summary>{t.status === 'complete' ? '✓' : t.status === 'blocked' ? '▲' : t.status === 'waiting' ? '◇' : '○'} {t.input.title} · {t.status === 'complete' ? 'reported complete' : t.status === 'waiting' ? 'waiting for you' : t.status} · {t.stage} · {t.history.length} steps{t.input.id !== 'assembly' && ` · ${t.input.mode ?? 'independent'}${(t.input.depends_on ?? []).some(id => run.tasks.find(p => p.input.id === id)?.status !== 'complete') ? ' · waiting for topics' : ''}`}</summary>
    {agent && seatIdFor(draft, agent) && <Link to={agentPath(run.rig, seatIdFor(draft, agent)!)}>Open responsible agent transcript</Link>}
    <p className="workflow-context">{t.input.context}</p>{t.copy && <p>Item folder: <code>{t.copy}</code></p>}{t.problem && <p role="status">{t.problem}</p>}
    {t.input.id !== 'assembly' && <TopicExecution key={`${t.input.mode}:${t.input.depends_on?.join(',')}`} task={t} run={run} active={active} />}
    {t.assignment && <p>Assignment {t.assignment}. If delivery is unconfirmed, inspect the agent transcript before restarting; it is never silently sent twice.</p>}
    <ol>{t.history.map(h => <li key={h.assignment}><strong>{h.stage} → {h.outcome}</strong> · {h.by === 'person' ? 'a person' : h.by === 'check' ? 'Sculptured ran the check' : h.by === 'sculptured' ? 'Sculptured' : h.agent === 'panel' ? 'review panel' : h.agent} · <time>{new Date(h.at_ms).toLocaleString()}</time> · {provenance(h)}{stepTokens(h, usage) != null && ` · ${stepTokens(h, usage)!.toLocaleString()} input + output tokens (CLI-reported)`}{stepCost(h, usage) && <span title={costTitle(stepCost(h, usage)!.sources)}>{` · ${costText(stepCost(h, usage)!.usd, false)} (estimate)`}</span>}{Object.keys(h.votes ?? {}).length > 0 && <p className="workflow-votes">{Object.entries(h.votes ?? {}).map(([a, o]) => `${a}: ${o}`).join(' · ')}</p>}<pre>{h.result}</pre>{h.evidence && <pre className="workflow-evidence">{h.evidence}</pre>}</li>)}</ol>
    {active && t.status === 'blocked' && <button className="btn" onClick={() => void retry()}>Retry this stage after inspecting the issue</button>}{error && <DismissibleError>{error}</DismissibleError>}
  </details>;
}

/** A step's reported tokens: its own turn's, or for a review panel every reviewer's; null when none reported. */
function stepTokens(h: WorkflowStep, usage: Record<string, TurnTokens>): number | null {
  const reported = [h.assignment, ...(h.panel ?? []).flatMap((r) => (r.assignment ? [r.assignment] : []))].flatMap((a) => (usage[a] ? [usage[a].input + usage[a].output] : []));
  return reported.length ? reported.reduce((a, b) => a + b, 0) : null;
}

/** A step's estimated cost (its own turn's, or every reviewer's); null when nothing priced it. */
function stepCost(h: WorkflowStep, usage: Record<string, TurnTokens>): { usd: number; sources: CostSource[] } | null {
  const priced = [h.assignment, ...(h.panel ?? []).flatMap((r) => (r.assignment ? [r.assignment] : []))].flatMap((a) => (usage[a]?.cost_usd != null ? [usage[a]] : []));
  return priced.length ? { usd: priced.reduce((sum, u) => sum + u.cost_usd!, 0), sources: [...new Set(priced.flatMap((u) => (u.cost_source ? [u.cost_source] : [])))] } : null;
}

/** The guard's last run failed: new work is held until it passes, or a person resumes anyway. */
function GuardNotice({ run, active, onResumed }: { run: WorkflowRun; active: boolean; onResumed: () => void }) {
  const g = run.guard!, [error, setError] = useState('');
  const resume = async () => { try { await api.resumeGuard(run.rig, run.start_seq); setError(''); onResumed(); } catch (e) { setError(errorText(e)); } };
  return <section className="guard-notice" aria-label="Guard">
    <strong role="status">{g.resumed_by_person ? 'Resumed by a person past a failing guard' : 'Paused: the guard failed'} at {new Date(g.at_ms).toLocaleTimeString()}</strong>
    <p className="muted">Verified: Sculptured ran the guard command. Work already under way finishes; no new work goes out until it passes.</p>
    <pre className="workflow-evidence">{g.evidence}</pre>
    {active && !g.resumed_by_person && <button className="btn" onClick={() => void resume()}>Resume anyway</button>}
    {error && <DismissibleError>{error}</DismissibleError>}
  </section>;
}
