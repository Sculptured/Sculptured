import { useState } from 'react';
import { api } from '../api';
import { errorText } from '../errors';
import type { DispatchRecord } from '../generated/DispatchRecord';
import type { Workflow } from '../generated/Workflow';
import type { Box } from '../layout';
import { DismissibleError } from '../shell/DismissibleError';

/** The board node for a workflow's dispatch command, drawn where a dispatcher agent would hand out the work. */
export const DISPATCH_NODE = '$dispatch-command';

/** Whether Sculptured makes the batch by running a command, instead of a dispatcher agent. */
export const byCommand = (w: Workflow) => !!w.dispatch && !w.dispatcher;

/** Advanced workflow settings: the dispatch command and its time limit. */
export function DispatchCommandFields({ w, locked, change }: { w: Workflow; locked: boolean; change: (w: Workflow) => void }) {
  const set = (command: string) => change(command ? { ...w, dispatcher: '', dispatch: { ...w.dispatch, command } } : { ...w, dispatch: undefined });
  return <>
    <label>Dispatch command <input className="mono" value={w.dispatch?.command ?? ''} disabled={locked} maxLength={2000} placeholder="e.g. cat plan/tasks.json" onChange={e => set(e.target.value)} /></label>
    {w.dispatch && <label>Dispatch command time limit in seconds <input type="number" min={1} max={3600} placeholder="600" value={w.dispatch.timeout_s ?? ''} disabled={locked} onChange={e => change({ ...w, dispatch: { ...w.dispatch!, timeout_s: e.target.value ? Number(e.target.value) : undefined } })} /></label>}
    <p className="muted">Instead of a dispatcher agent, Sculptured runs this command once in the project folder when the team starts, and what it prints is the work: JSON with a summary and tasks, each with an id, title and context. Leave it empty to choose a dispatcher agent.</p>
  </>;
}

/** The dispatch command on the board, in place of the dispatcher agent. Opens the workflow settings. */
export function DispatchBox({ w, box, onOpen, onReveal }: { w: Workflow; box: Box; onOpen: () => void; onReveal: (b: Box) => void }) {
  return <button className="workflow-stage-node kind-dispatch" style={{ left: box.x, top: box.y, width: box.w, height: box.h }}
    aria-label="Dispatch command: Sculptured runs it to make the work" onFocus={() => onReveal(box)} onClick={onOpen}>
    <strong>Dispatch command</strong><span>Sculptured runs it when the team starts</span><p className="mono">$ {w.dispatch?.command ?? ''}</p>
  </button>;
}

/** What came of the dispatch command for this start: running, dispatched, or nothing dispatched and why. */
export function DispatchNotice({ team, start, record, running, active, onRetried }: { team: string; start: number | null; record: DispatchRecord | null | undefined; running: boolean; active: boolean; onRetried: () => void }) {
  const [error, setError] = useState('');
  if (running) return <p role="status" className="muted">◌ Running the dispatch command…</p>;
  if (!record) return null;
  const retry = async () => { if (start == null) return; try { await api.retryDispatch(team, start); setError(''); onRetried(); } catch (e) { setError(errorText(e)); } };
  return <section className={`guard-notice${record.dispatched ? ' is-dispatched' : ''}`} aria-label="Dispatch command">
    <strong role="status">{record.dispatched ? '✓ Work made by the dispatch command' : '▲ Blocked: the dispatch command made no work'} at {new Date(record.at_ms).toLocaleTimeString()}</strong>
    {record.problem && <p data-raw-evidence>{record.problem}</p>}
    <p className="muted">Observed: Sculptured ran the command{record.dispatched ? '; the items are what it printed.' : '.'}</p>
    <pre className="workflow-evidence" data-raw-evidence>{record.evidence}</pre>
    {active && !record.dispatched && <button className="btn" disabled={start == null} onClick={() => void retry()}>Run the dispatch command again</button>}
    {error && <DismissibleError>{error}</DismissibleError>}
  </section>;
}
