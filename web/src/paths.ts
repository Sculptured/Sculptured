/**
 * Every page's address. A project (team) lives under `/p/:team`, one page per job:
 * Board, Agents, an Agent (Conversation, Setup, Activity, Questions), Questions, Results and Settings.
 */

const enc = encodeURIComponent;

/** The user guides and reference, on their own site. */
export const DOCS_URL = 'https://docs.usesculptured.com';

export type BoardMode = 'design' | 'run';
export type AgentTab = 'conversation' | 'setup' | 'activity' | 'questions';
export type ResultsSection = 'summary' | 'conversation' | 'usage' | 'communications' | 'knowledge' | 'history';

export const RESULTS_SECTIONS: { id: ResultsSection; label: string }[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'conversation', label: 'Team conversation' },
  { id: 'usage', label: 'Usage' },
  { id: 'communications', label: 'Communications' },
  { id: 'knowledge', label: 'Knowledge' },
  { id: 'history', label: 'History' },
];

export const boardPath = (team: string, mode?: BoardMode) => `/p/${enc(team)}/board${mode ? `?mode=${mode}` : ''}`;
export const agentsPath = (team: string) => `/p/${enc(team)}/agents`;
export const settingsPath = (team: string) => `/p/${enc(team)}/settings`;
/** Every question the team's agents asked: open ones, answers, choices and the agents' own decisions. */
export const questionsPath = (team: string) => `/p/${enc(team)}/questions`;
export const resultsPath = (team: string, section?: ResultsSection) =>
  `/p/${enc(team)}/results${section && section !== 'summary' ? `/${section}` : ''}`;
/** A workflow's own page: `input`, `settings`, `run`, `list` or `stage:<id>`. */
export const workflowPath = (team: string, target: string) => `/p/${enc(team)}/workflow/${enc(target)}`;

/** An agent's address inside its team: its id without the trailing `@team`. */
export function agentKey(team: string, seat: string): string {
  const tail = `@${team}`;
  return seat.endsWith(tail) ? seat.slice(0, -tail.length) : seat;
}

/** The agent's full id back from its address. */
export const seatFromKey = (team: string, key: string) => (key.includes('@') ? key : `${key}@${team}`);

export const agentPagePath = (team: string, seat: string, tab?: AgentTab) =>
  `/p/${enc(team)}/agents/${enc(agentKey(team, seat))}${tab ? `/${tab}` : ''}`;

/** Old agent tab names (`/rig/…/agent/…/visual`) to the new tabs. */
export function agentTabOf(old: string | undefined): { tab?: AgentTab; search: string } {
  if (old === 'visual') return { tab: 'activity', search: '' };
  if (old === 'events') return { tab: 'activity', search: '?view=events' };
  if (old === 'setup' || old === 'activity' || old === 'conversation') return { tab: old, search: '' };
  return { search: '' };
}
