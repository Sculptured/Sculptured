import { createRegistry, type PaneType } from '../workspace/registry';
import type { ControlCtx } from './context';
import { ACTIONS_DEFAULTS, ActionsPane, ActionsSettingsForm } from './panes/Actions';
import { AgentDetailPane, AgentSettingsForm } from './panes/AgentDetail';
import { AlarmsPane, AlarmsSettingsForm } from './panes/Alarms';
import { ApprovalsPane } from './panes/Approvals';
import { ComparePane } from './panes/Compare';
import { MetricsPane } from './panes/Metrics';
import { OverviewPane } from './panes/Overview';
import { ProcessPane } from './panes/Process';
import { RawEventsPane, RawSettingsForm } from './panes/RawEvents';
import { ResultsPane, ResultsSettingsForm } from './panes/Results';
import { STATUS_DEFAULTS, StatusPane, StatusSettingsForm } from './panes/Status';
import { TIMELINE_DEFAULTS, TimelinePane, TimelineSettingsForm } from './panes/Timeline';
import { TrendsPane } from './panes/Trends';
import { WorldPane } from './panes/World';
import { gated } from './SimGate';
import './plant/plant.css';

const deploymentOnly = (ctx: unknown) => (ctx as ControlCtx | undefined)?.kind === 'deployment';
const simulatedOnly = (ctx: unknown) => (ctx as ControlCtx | undefined)?.kind === 'simulated';

/** Control's pane types: one list for both kinds of deployment, each type saying where it applies. */
const PANES: PaneType[] = [
  { type: 'overview', title: 'System overview', glyph: '◫', selection: true, Body: OverviewPane },
  { type: 'status', title: 'Status', glyph: '≡', selection: true, defaultSettings: STATUS_DEFAULTS, Settings: StatusSettingsForm, Body: StatusPane },
  { type: 'timeline', title: 'Timeline', glyph: '⏱', selection: true, defaultSettings: TIMELINE_DEFAULTS, Settings: TimelineSettingsForm, Body: TimelinePane },
  // The control plane: the process view (level 2), the action timeline with each action's trace (level 4), trends.
  { type: 'process', title: 'Process view', glyph: '⊞', selection: true, Body: ProcessPane },
  { type: 'actions', title: 'Action timeline', glyph: '≣', selection: true, defaultSettings: ACTIONS_DEFAULTS, Settings: ActionsSettingsForm, Body: ActionsPane },
  { type: 'trends', title: 'Trends', glyph: '∿', Body: TrendsPane },
  { type: 'alarms', title: 'Alarms', glyph: '▲', selection: true, Settings: AlarmsSettingsForm, Body: AlarmsPane },
  { type: 'approvals', title: 'Approvals', glyph: '◆', Body: ApprovalsPane },
  // Agent detail has its own Follow selection switch in its head (it pins the agent), so it does not take
  // the shell's generic "Follow selection" setting (`selection: true`), which other panes keep.
  { type: 'agent', title: 'Agent detail', glyph: '◉', multi: true, appliesTo: deploymentOnly, defaultSettings: { view: 'conversation' }, Settings: AgentSettingsForm, Body: AgentDetailPane },
  { type: 'metrics', title: 'Metrics', glyph: '▥', Body: MetricsPane },
  { type: 'raw', title: 'Raw events', glyph: '{}', selection: true, defaultSettings: { onlySelected: false }, Settings: RawSettingsForm, Body: RawEventsPane },
  { type: 'results', title: 'Results', glyph: '✓', multi: true, appliesTo: deploymentOnly, defaultSettings: { section: 'summary' }, Settings: ResultsSettingsForm, Body: ResultsPane },
  { type: 'world', title: 'World state', glyph: '◎', selection: true, appliesTo: simulatedOnly, Body: WorldPane },
  { type: 'compare', title: 'Compare', glyph: '⇄', multi: true, appliesTo: simulatedOnly, defaultSettings: {}, Body: ComparePane },
];

/** Every pane of a simulated run waits for its branch (saying why it can't show while Arena is not answering). */
export const CONTROL_PANES: PaneType[] = PANES.map((t) => ({ ...t, Body: gated(t.Body) }));

/** Made once, here: the workspace keeps the wrapped types by identity. */
export const controlRegistry = createRegistry(CONTROL_PANES);
