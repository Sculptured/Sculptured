import { Link, useNavigate, useParams } from 'react-router-dom';
import { CommunicationDashboard } from '../collaboration/CommunicationDashboard';
import { DeploymentHistoryView } from '../collaboration/DeploymentHistoryView';
import { KnowledgeView } from '../collaboration/KnowledgeView';
import { StatusPageView } from '../collaboration/StatusPageView';
import { TeamConversation } from '../collaboration/TeamConversation';
import { UsageDashboard } from '../collaboration/UsageDashboard';
import { WhereTokensWent } from '../collaboration/WhereTokensWent';
import { SpecNotes, specNotes } from '../components/SpecNotes';
import { RESULTS_SECTIONS, resultsPath, workflowPath, type ResultsSection } from '../paths';
import type { AgentRow } from '../shell/model';
import { RunSummary } from '../shell/RunFinished';
import { useShell } from '../shell/ShellContext';
import { UsageStrip } from '../shell/UsageStrip';
import { Icon } from '../shell/ui';
import { WORDS } from '../vocab';
import { useProject, type Project } from './ProjectContext';
import { ChangesView } from '../collaboration/ChangesView';

/** Results' own pages, as a row of plain tabs. */
function SubTabs({ team, current }: { team: string; current: ResultsSection }) {
  return (
    <nav className="sub-tabs" aria-label="Results">
      {RESULTS_SECTIONS.map((s) => (
        <Link key={s.id} to={resultsPath(team, s.id)} className={`sub-tab${current === s.id ? ' on' : ''}`} aria-current={current === s.id ? 'page' : undefined}>{s.label}</Link>
      ))}
    </nav>
  );
}

/** The live agents in team order. */
function agentsOf(p: Project, byId: Record<string, AgentRow>): Record<string, AgentRow> {
  const out: Record<string, AgentRow> = {};
  for (const s of p.live?.snapshot?.rig.seats ?? []) if (byId[s.id]) out[s.id] = byId[s.id];
  return out;
}

/**
 * `/p/:team/results[/:section]`: what the team produced. Summary first (what each agent reported,
 * files changed, checks, usage); the team conversation, usage detail, communications, knowledge and
 * run history each have their own tab.
 */
export function ResultsPage() {
  const p = useProject();
  const navigate = useNavigate();
  const { section } = useParams();
  const current = (RESULTS_SECTIONS.find((s) => s.id === section)?.id ?? 'summary') as ResultsSection;
  // History, Status and Changes need no live team: a team that never ran here can hold an imported run,
  // and the status page and the changes are kept after the team stops.
  if (KEPT.includes(current) && !p.live)
    return (
      <main className={`page page-wide results-page${current === 'history' ? ' results-history' : ''}`}>
        <SubTabs team={p.team} current={current} />
        <Kept team={p.team} current={current} onClose={() => navigate(resultsPath(p.team))} />
      </main>
    );
  if (!p.started)
    return (
      <main className="page page-wide results-page">
        <section className="empty-state" aria-label="Results">
          <Icon name="check-circle" size={22} className="muted" />
          <h2>No runs yet</h2>
          <p className="muted">Start the team to see what it produced: each agent's report, files changed and checks.</p>
        </section>
      </main>
    );
  return (
    <main className={`page page-wide results-page${current === 'history' ? ' results-history' : ''}`}>
      <SubTabs team={p.team} current={current} />
      {!p.live ? <p role="status" className="muted">{WORDS.connecting(p.title)}</p> : <Section p={p} current={current} />}
    </main>
  );
}

/** Tabs that read what is kept, so they show without a live team. */
const KEPT: ResultsSection[] = ['history', 'status', 'changes'];

function Kept({ team, current, onClose }: { team: string; current: ResultsSection; onClose: () => void }) {
  if (current === 'history') return <DeploymentHistoryView rig={team} />;
  if (current === 'changes') return <ChangesView rig={team} />;
  return <StatusPageView rig={team} onClose={onClose} />;
}

function Section({ p, current }: { p: Project; current: ResultsSection }) {
  const shell = useShell();
  const navigate = useNavigate();
  const state = p.live!;
  const agents = agentsOf(p, shell.byId);
  const names = p.row?.names ?? {};
  const back = () => navigate(resultsPath(p.team));
  const open = (id: string) => shell.open({ rig: p.team, id });
  switch (current) {
    case 'conversation':
      return <TeamConversation key={p.team} rig={p.team} state={state} connected={p.connected} names={names} agents={agents} onOpen={open} onAnswer={(id) => shell.answer(p.team, id)} onClose={back} />;
    case 'usage':
      return <div className="results-stack"><UsageDashboard rig={p.team} names={names} onClose={back} /><WhereTokensWent rig={p.team} seats={Object.keys(agents)} onClose={back} /></div>;
    case 'communications':
      return <CommunicationDashboard rig={p.team} state={state} connected={p.connected} names={names} onClose={back} />;
    case 'knowledge':
      return <KnowledgeView rig={p.team} onClose={back} />;
    case 'status':
      return <StatusPageView rig={p.team} onClose={back} />;
    case 'changes':
      return <ChangesView rig={p.team} />;
    case 'history':
      return <DeploymentHistoryView rig={p.team} />;
    default:
      return <Summary p={p} agents={agents} />;
  }
}

function Summary({ p, agents }: { p: Project; agents: Record<string, AgentRow> }) {
  const shell = useShell();
  const snap = p.live!.snapshot!;
  const views = snap.rig.seats.map((s) => p.live!.seats[s.id]).filter(Boolean);
  const notes = specNotes(snap.rig.diagnostics);
  return (
    <div className="results-summary">
      <RunSummary rig={p.team} agents={Object.values(agents)} teamId={p.builtHere ? p.team : undefined} folder={p.draft ? (p.draft.folder ?? null) : undefined} onOpen={(id) => shell.open({ rig: p.team, id })} />
      <section className="rs-sec" aria-label="Usage this run">
        <h3>Usage this run</h3>
        <UsageStrip rig={p.team} seats={views} />
      </section>
      {p.draft?.workflow && <p><Link to={workflowPath(p.team, 'run')}>Work evidence: assembly and each item's review</Link></p>}
      {notes.length > 0 && (
        <details className="spec-advanced" data-advanced>
          <summary>{WORDS.setupNotes}</summary>
          <SpecNotes diagnostics={snap.rig.diagnostics} />
        </details>
      )}
    </div>
  );
}
