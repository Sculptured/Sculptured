import { useCallback, useMemo, useState } from 'react';
import { draftLayout } from '../draft/geometry';
import { nameMap } from '../draft/ids';
import type { TeamDraft } from '../generated/TeamDraft';
import { STATES } from '../herdr';
import type { Layout } from '../layout';
import type { OpenStream } from '../useRigStream';
import { useTeamList } from '../useTeamList';
import { useActivity, useTeams } from '../useTeams';
import type { NameMap } from '../vocab';
import { buildTeams, draftTeamRow, firstMessageIsWork, sortTeams, type AgentRow, type TeamRow } from './model';

/** Names and positions from each team's design: they win over the spec's labels on the Run board too. */
function useDesigns(drafts: Record<string, TeamDraft>, givenNames: Record<string, NameMap>, givenLayouts: Record<string, Layout>) {
  const names = useMemo(() => {
    const out: Record<string, NameMap> = { ...givenNames };
    for (const [id, d] of Object.entries(drafts)) out[id] = { ...nameMap(d), ...(givenNames[id] ?? {}) };
    return out;
  }, [drafts, givenNames]);
  const layouts = useMemo(() => {
    const out: Record<string, Layout> = {};
    for (const [id, d] of Object.entries(drafts)) out[id] = draftLayout(d);
    return { ...out, ...givenLayouts };
  }, [drafts, givenLayouts]);
  const titles = useMemo(() => Object.fromEntries(Object.entries(drafts).map(([id, d]) => [id, d.name.trim() || id])), [drafts]);
  return { names, layouts, titles };
}

/** Every team (live or a design not started yet) and its agents, most active first. */
export function useShellData(team: string | undefined, openStream: OpenStream | undefined, givenLayouts: Record<string, Layout>, givenNames: Record<string, NameMap>, openAgent: string | undefined, seenVersion: number) {
  const teamList = useTeamList();
  const startedTeams = useMemo(() => (teamList.summaries ?? []).filter((s) => s.started).map((s) => s.id), [teamList.summaries]);
  // A design that has never started has no live stream to open.
  const draftOnly = !!team && !!teamList.summaries?.some((s) => s.id === team && !s.started);
  const { rigs, listError, teams, names: order } = useTeams(draftOnly ? undefined : team, openStream, startedTeams);
  // The open project's design wins over the last one read from the list.
  const [openDrafts, setOpenDrafts] = useState<Record<string, TeamDraft>>({});
  const onDraft = useCallback((d: TeamDraft) => setOpenDrafts((cur) => (cur[d.id] === d ? cur : { ...cur, [d.id]: d })), []);
  const drafts = useMemo(() => ({ ...teamList.drafts, ...openDrafts }), [teamList.drafts, openDrafts]);
  const { names, layouts, titles } = useDesigns(drafts, givenNames, givenLayouts);
  const activity = useActivity(teams);
  const rows = useMemo(
    () => teamRows({ teams, order, activity, names, titles, drafts, summaries: teamList.summaries, rigs, openAgent }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- seenVersion: an agent was viewed, so its "done" clears
    [teams, order, activity, names, titles, teamList.summaries, drafts, rigs, seenVersion, openAgent],
  );
  const byId = useMemo(() => {
    const m: Record<string, AgentRow> = {};
    for (const t of rows) for (const a of t.agents) m[a.id] = a;
    return m;
  }, [rows]);
  return { teamList, rigs, listError, teams, drafts, onDraft, names, layouts, titles, activity, rows, byId };
}

type RowInput = {
  teams: ReturnType<typeof useTeams>['teams'];
  order: string[];
  activity: ReturnType<typeof useActivity>;
  names: Record<string, NameMap>;
  titles: Record<string, string>;
  drafts: Record<string, TeamDraft>;
  summaries: ReturnType<typeof useTeamList>['summaries'];
  rigs: ReturnType<typeof useTeams>['rigs'];
  openAgent: string | undefined;
};

function teamRows(p: RowInput): TeamRow[] {
  const built = buildTeams(p.teams, p.order, p.activity, p.names, p.titles);
  // A team started here runs its agents without a folder in the home folder ("." from home).
  for (const t of built) if (p.drafts[t.rig]) for (const a of t.agents) if (a.folder === '.') a.folder = '~';
  // A first agent with no Job: your next message is its work (shown above its message box).
  for (const t of built) {
    const live = p.teams[t.rig]?.state;
    if (!live?.snapshot) continue;
    for (const a of t.agents) a.firstMessageIsWork = !!a.teamRunning && firstMessageIsWork(live.snapshot.rig, live.seats, p.drafts[t.rig], a.id);
  }
  // Teams built here that have never started: their agents come from the draft.
  for (const s of p.summaries ?? []) {
    if (p.order.includes(s.id) || s.started) continue;
    const d = p.drafts[s.id];
    if (d) built.push(draftTeamRow(d));
  }
  // The agent you are looking at has nothing unseen.
  for (const t of built) for (const a of t.agents) if (a.id === p.openAgent && a.state.key === 'done') a.state = { ...a.state, ...STATES.idle };
  return sortTeams(built, p.rigs);
}
