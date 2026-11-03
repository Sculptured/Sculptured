import { CommunicationDashboard } from '../../collaboration/CommunicationDashboard';
import { ChangesView } from '../../collaboration/ChangesView';
import { DeploymentHistoryView } from '../../collaboration/DeploymentHistoryView';
import { KnowledgeView } from '../../collaboration/KnowledgeView';
import { StatusPageView } from '../../collaboration/StatusPageView';
import { TeamConversation } from '../../collaboration/TeamConversation';
import { UsageDashboard } from '../../collaboration/UsageDashboard';
import { WhereTokensWent } from '../../collaboration/WhereTokensWent';
import { useProject, type Project } from '../../pages/ProjectContext';
import { RESULTS_SECTIONS, type ResultsSection } from '../../paths';
import type { AgentRow } from '../../shell/model';
import { RunSummary } from '../../shell/RunFinished';
import { useShell } from '../../shell/ShellContext';
import { UsageStrip } from '../../shell/UsageStrip';
import type { PaneProps } from '../../workspace/registry';
import { useControl } from '../context';

/** Results: one of the Results page's sections, chosen in Settings (Summary by default). */

export type ResultsSettings = { section?: ResultsSection };
/** Sections that read what is kept, so they show without a live team. */
const KEPT: ResultsSection[] = ['history', 'status', 'changes'];
const noop = () => {};

function agentsOf(p: Project, byId: Record<string, AgentRow>): Record<string, AgentRow> {
  const out: Record<string, AgentRow> = {};
  for (const s of p.live?.snapshot?.rig.seats ?? []) if (byId[s.id]) out[s.id] = byId[s.id];
  return out;
}

function Kept({ team, section }: { team: string; section: ResultsSection }) {
  if (section === 'history') return <DeploymentHistoryView rig={team} />;
  if (section === 'changes') return <ChangesView rig={team} />;
  return <StatusPageView rig={team} onClose={noop} />;
}

function Summary({ p, agents }: { p: Project; agents: Record<string, AgentRow> }) {
  const shell = useShell();
  const views = p.live!.snapshot!.rig.seats.map((s) => p.live!.seats[s.id]).filter(Boolean);
  return (
    <div className="results-summary">
      <RunSummary rig={p.team} agents={Object.values(agents)} teamId={p.builtHere ? p.team : undefined} folder={p.draft ? (p.draft.folder ?? null) : undefined} onOpen={(id) => shell.open({ rig: p.team, id })} />
      <section className="rs-sec" aria-label="Usage this run"><h3>Usage this run</h3><UsageStrip rig={p.team} seats={views} /></section>
    </div>
  );
}

function Live({ p, section }: { p: Project; section: ResultsSection }) {
  const shell = useShell();
  const { show } = useControl();
  const agents = agentsOf(p, shell.byId);
  const names = p.row?.names ?? {};
  const state = p.live!;
  switch (section) {
    case 'conversation':
      return <TeamConversation rig={p.team} state={state} connected={p.connected} names={names} agents={agents} onOpen={(id) => shell.open({ rig: p.team, id })} onAnswer={() => show('approvals')} onClose={noop} />;
    case 'usage':
      return <div className="results-stack"><UsageDashboard rig={p.team} names={names} onClose={noop} /><WhereTokensWent rig={p.team} seats={Object.keys(agents)} onClose={noop} /></div>;
    case 'communications':
      return <CommunicationDashboard rig={p.team} state={state} connected={p.connected} names={names} onClose={noop} />;
    case 'knowledge':
      return <KnowledgeView rig={p.team} onClose={noop} />;
    default:
      return KEPT.includes(section) ? <Kept team={p.team} section={section} /> : <Summary p={p} agents={agents} />;
  }
}

export function ResultsPane({ settings }: PaneProps) {
  const section = (settings as ResultsSettings).section ?? 'summary';
  const p = useProject();
  let body;
  if (KEPT.includes(section) && !p.live) body = <Kept team={p.team} section={section} />;
  else if (!p.started) body = <p className="ctl-empty">No runs yet. What the team produced shows here once it has run.</p>;
  else if (!p.live) body = <p className="ctl-empty" role="status">Connecting…</p>;
  else body = <Live p={p} section={section} />;
  return <div className="ctl-results" data-section={section}>{body}</div>;
}

export function ResultsSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as ResultsSettings;
  return (
    <label className="ctl-field">Section
      <select value={s.section ?? 'summary'} onChange={(e) => setSettings({ ...s, section: e.target.value as ResultsSection })}>
        {RESULTS_SECTIONS.map((r) => <option key={r.id} value={r.id}>{r.label}</option>)}
      </select>
    </label>
  );
}
