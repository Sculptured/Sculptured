import { createRegistry, type PaneType } from '../workspace/registry';
import type { ControlCtx } from './context';
import { AgentDetailPane, AgentSettingsForm } from './panes/AgentDetail';
import { AlarmsPane, AlarmsSettingsForm } from './panes/Alarms';
import { ApprovalsPane } from './panes/Approvals';
import { MetricsPane } from './panes/Metrics';
import { OverviewPane } from './panes/Overview';
import { RawEventsPane, RawSettingsForm } from './panes/RawEvents';
import { ResultsPane, ResultsSettingsForm } from './panes/Results';
import { STATUS_DEFAULTS, StatusPane, StatusSettingsForm } from './panes/Status';
import { TIMELINE_DEFAULTS, TimelinePane, TimelineSettingsForm } from './panes/Timeline';

const deploymentOnly = (ctx: unknown) => (ctx as ControlCtx | undefined)?.kind === 'deployment';

/** Control's pane types: one list for both kinds of deployment, each type saying where it applies. */
export const CONTROL_PANES: PaneType[] = [
  { type: 'overview', title: 'System overview', glyph: '◫', selection: true, appliesTo: deploymentOnly, Body: OverviewPane },
  { type: 'status', title: 'Status', glyph: '≡', selection: true, defaultSettings: STATUS_DEFAULTS, Settings: StatusSettingsForm, Body: StatusPane },
  { type: 'timeline', title: 'Timeline', glyph: '⏱', selection: true, defaultSettings: TIMELINE_DEFAULTS, Settings: TimelineSettingsForm, Body: TimelinePane },
  { type: 'alarms', title: 'Alarms', glyph: '▲', selection: true, Settings: AlarmsSettingsForm, Body: AlarmsPane },
  { type: 'approvals', title: 'Approvals', glyph: '◆', Body: ApprovalsPane },
  { type: 'agent', title: 'Agent detail', glyph: '◉', multi: true, selection: true, appliesTo: deploymentOnly, defaultSettings: { view: 'conversation' }, Settings: AgentSettingsForm, Body: AgentDetailPane },
  { type: 'metrics', title: 'Metrics', glyph: '▥', Body: MetricsPane },
  { type: 'raw', title: 'Raw events', glyph: '{}', selection: true, defaultSettings: { onlySelected: false }, Settings: RawSettingsForm, Body: RawEventsPane },
  { type: 'results', title: 'Results', glyph: '✓', multi: true, appliesTo: deploymentOnly, defaultSettings: { section: 'summary' }, Settings: ResultsSettingsForm, Body: ResultsPane },
];

/** Made once, here: the workspace keeps the wrapped types by identity. Task 8 adds `world` and `compare`. */
export const controlRegistry = createRegistry(CONTROL_PANES);
