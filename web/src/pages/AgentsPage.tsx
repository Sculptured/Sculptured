import { useId } from 'react';
import { useNavigate } from 'react-router-dom';
import { seatIdFor } from '../draft/ids';
import { effectiveModel } from '../draft/models';
import type { TeamDraft } from '../generated/TeamDraft';
import { shortAge } from '../herdr';
import { canReceive } from '../shell/Broadcast';
import type { AgentRow } from '../shell/model';
import { lastReply } from '../shell/runFinished';
import { useShell } from '../shell/ShellContext';
import { Btn, Icon, StateMark, StuckMark, VendorMark, WaitingMark } from '../shell/ui';
import { shortPath, useHome } from '../structure/FolderPicker';
import { modelLabel } from '../structure/ModelControls';
import { edgeSentence, KIND_LABEL } from '../vocab';
import { needsAttention } from '../status';
import { connectionSentence } from '../structure/Panels';
import { agentPagePath } from '../paths';
import { useProject } from './ProjectContext';

/** The team's agents in its own order: the design's, else the started team's. */
function ordered(rows: AgentRow[], draft: TeamDraft | null, seats: string[] | undefined): AgentRow[] {
  const order = draft ? draft.agents.map((a) => seatIdFor(draft, a.id)!) : (seats ?? []);
  const at = (id: string) => { const i = order.indexOf(id); return i < 0 ? order.length : i; };
  return [...rows].sort((a, b) => at(a.id) - at(b.id));
}

/** "Claude · Sonnet 4.5", from the design when there is one, else as the agent reported it. */
function runsOn(a: AgentRow, draft: TeamDraft | null): string {
  const d = draft?.agents.find((x) => seatIdFor(draft, x.id) === a.id);
  if (draft && d) return d.kind === 'practice' ? 'Practice · free' : `${KIND_LABEL[d.kind]} · ${modelLabel(d.kind, effectiveModel(draft, d).model)}`;
  const model = a.seat?.model_observed ?? a.seat?.model_requested;
  return `${a.vendor.name}${model ? ` · ${model}` : ''}`;
}

/** Arrow keys (and j, k) move between rows; Shift+Space picks the focused one. */
function rowKeys(e: React.KeyboardEvent<HTMLTableSectionElement>, onPick: (id: string) => void) {
  const row = (e.target as HTMLElement).closest<HTMLElement>('tr[data-agent]');
  if (!row) return;
  if (e.key === ' ' && e.shiftKey) { e.preventDefault(); onPick(row.dataset.agent!); return; }
  const step = e.key === 'ArrowDown' || e.key === 'j' ? 1 : e.key === 'ArrowUp' || e.key === 'k' ? -1 : 0;
  if (!step || (e.target as HTMLElement).matches('input')) return;
  e.preventDefault();
  const next = (step > 0 ? row.nextElementSibling : row.previousElementSibling) as HTMLElement | null;
  next?.querySelector<HTMLElement>('a.at-name')?.focus();
}

/**
 * `/p/:team/agents`: every agent as one row of a plain table. A row opens the agent; tick rows (or
 * Shift+click) to message several at once.
 */
export function AgentsPage() {
  const p = useProject();
  const shell = useShell();
  const ids = useId();
  const home = useHome();
  const navigate = useNavigate();
  const rows = ordered(p.row?.agents ?? [], p.draft, p.live?.snapshot?.rig.seats.map((s) => s.id));
  const picked = new Set(shell.picked);
  const receivers = rows.filter(canReceive).length;
  const none = receivers === 0 ? (p.started ? 'No agent is running.' : 'Start the team first.') : null;
  const open = (a: AgentRow) => navigate(linkOf(p.team, a));
  return (
    <main className="page page-list agents-page">
      <div className="list-head-row">
        <h2 className="page-title">Agents <span className="muted count">{rows.length}</span></h2>
        <span className="tb-fill" />
        {picked.size > 0 && <span className="muted" role="status">{picked.size} selected</span>}
        <Btn size="sm" icon="message" disabled={!!none || picked.size === 0} aria-describedby={`${ids}-why`} onClick={() => shell.broadcast(p.team, [...picked])}>Message selected…</Btn>
        <Btn size="sm" icon="message" disabled={!!none} aria-describedby={none ? `${ids}-why` : undefined} onClick={() => shell.broadcast(p.team, [])}>Message all…</Btn>
        <span id={`${ids}-why`} className="reason">{none ?? (picked.size === 0 ? 'Tick agents to message some of them.' : '')}</span>
      </div>
      {rows.length === 0 ? <p className="muted empty-line">{p.row ? 'This team has no agents yet. Add one on the board.' : 'Loading agents…'}</p> : (
        <div className="table-wrap">
          <table className="data-table agents-table">
            <thead>
              <tr><th className="col-pick"><span className="sr-only">Select</span></th><th>Name</th><th>Runs on</th><th>Status</th><th className="col-folder">Folder</th><th className="col-age">Last activity</th><th className="col-said">Last message</th></tr>
            </thead>
            <tbody onKeyDown={(e) => rowKeys(e, shell.pick)}>
              {rows.map((a) => (
                <tr key={a.id} data-agent={a.id} className={`${picked.has(a.id) ? 'picked' : ''}${shell.selected === a.id ? ' selected' : ''}`}
                  onClick={(e) => { if ((e.target as HTMLElement).closest('a, input')) return; if (e.shiftKey || e.metaKey || e.ctrlKey) shell.pick(a.id); else open(a); }}>
                  <td className="col-pick"><input type="checkbox" aria-label={`Select ${a.name}`} checked={picked.has(a.id)} onChange={() => shell.pick(a.id)} /></td>
                  <td><a className="at-name" href={linkOf(p.team, a)} onClick={(e) => { e.preventDefault(); if (e.shiftKey || e.metaKey || e.ctrlKey) shell.pick(a.id); else open(a); }}><VendorMark v={a.vendor} size="xs" />{a.name}</a></td>
                  <td className="muted">{runsOn(a, p.draft)}</td>
                  <td><StateMark state={a.state} pill />{a.stuck && a.state.key === 'stuck' ? <StuckMark reason={a.stuck.reason} className="at-sub" /> : a.waitingFor?.length ? <WaitingMark names={a.waitingFor} className="at-sub" /> : null}{a.seat && needsAttention(a.seat.lifecycle) && a.seat.lifecycle_reason && <span className="at-reason reason" data-raw-evidence>{a.seat.lifecycle_reason}</span>}</td>
                  <td className={`col-folder muted${a.folder ? ' mono' : ''}`} title={a.folder || undefined}>{a.folder ? (a.folder === '~' ? '~' : shortPath(a.folder, home)) : 'none chosen'}</td>
                  <td className="col-age muted">{a.seat ? shortAge(a.lastMs, shell.now) : '—'}</td>
                  <td className="col-said"><Said a={a} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <Connections />
    </main>
  );
}

/** Who hands work to whom, as sentences: the board's arrows in words. */
function Connections() {
  const p = useProject();
  const names = p.row?.names ?? {};
  const lines = p.draft
    ? p.draft.connections.map((c) => ({ key: c.id, text: connectionSentence(p.draft!, c) }))
    : (p.live?.snapshot?.rig.edges ?? []).map((e) => ({ key: `${e.kind}-${e.from}-${e.to}`, text: edgeSentence(e, names) }));
  return (
    <section className="agents-links" aria-label="Connections">
      <h3 className="set-title">Connections</h3>
      {lines.length === 0 ? <p className="muted">No connections.</p> : (
        <ul>{lines.map((l) => <li key={l.key}><Icon name="arrow-right" size={13} className="muted" />{l.text}</li>)}</ul>
      )}
    </section>
  );
}

const linkOf = (team: string, a: AgentRow) => a.href ?? agentPagePath(team, a.id);

/** The agent's latest reply in its own words (agent-reported); unknown stays unknown. */
function Said({ a }: { a: AgentRow }) {
  if (!a.seat) return <span className="muted">not started</span>;
  const said = lastReply(a, 120);
  if (!said) return <span className="ux-unknown">no reply captured</span>;
  return (
    <span className="said">
      <span className="ux-prov-mark" data-tip="agent-reported: the agent's own words"><Icon name="quote" size={12} /><span className="sr-only">agent-reported: </span></span>
      <span data-raw-evidence>{said}</span>
    </span>
  );
}
