import type { useNavigate } from 'react-router-dom';
import type { View } from '../keys';
import type { AgentTab } from '../paths';
import { hasProcess } from '../status';
import { openAppearance } from './AppearanceControl';
import { canReceive } from './Broadcast';
import { buildCommands, type CommandContext } from './commands';
import type { Scope } from './CommandPalette';
import type { Shell } from './ShellContext';

type Deps = {
  shell: Shell;
  view: View;
  team: string | undefined;
  openAgent: string | undefined;
  /** The open agent's tab, from the address. */
  agentTab?: AgentTab;
  navigate: ReturnType<typeof useNavigate>;
  setHelp: (b: boolean) => void;
  setGoto: (s: Scope | false) => void;
  back: () => void;
};

/** The open project as the palette sees it. */
function teamOf(d: Deps): CommandContext['team'] {
  const { shell: s, team } = d;
  if (!team) return undefined;
  const row = s.rows.find((r) => r.rig === team);
  const started = !!s.summaries?.find((x) => x.id === team)?.started || !!s.rigs?.some((r) => r.name === team);
  return { id: team, title: s.titles[team] ?? row?.title ?? team, builtHere: s.isTeam(team), started, running: !!row?.agents.some((a) => a.seat && hasProcess(a.seat.lifecycle)) };
}

/** The palette's commands for the screen as it is when the palette opens. */
export function useShellCommands(d: Deps): () => Command[] {
  return () => {
    const { shell: s, team, view } = d;
    const row = team ? s.rows.find((r) => r.rig === team) : undefined;
    const agentRow = d.openAgent ? s.byId[d.openAgent] : undefined;
    const controls = view === 'agent' ? s.refs.controls.current : null;
    return buildCommands({
      view,
      team: teamOf(d),
      board: view === 'structure' ? s.refs.board.current : null,
      run: team ? s.refs.run.current : null,
      picked: s.picked.length,
      runningAgents: row?.agents.filter(canReceive).length ?? 0,
      agent: d.openAgent ? { id: d.openAgent, tab: d.agentTab ?? (agentRow?.seat ? 'conversation' : 'setup'), draftOnly: !agentRow?.seat } : undefined,
      interruptReason: controls ? controls.interruptReason : view === 'agent' ? 'Not started yet.' : null,
      handOver: controls?.handOver ?? null,
      projects: s.rows.filter((r) => r.rig !== team).map((r) => ({ id: r.rig, title: r.title, href: r.href })),
      go: (to) => d.navigate(to),
      act: {
        help: () => d.setHelp(true),
        appearance: openAppearance,
        fit: () => (view === 'structure' ? s.refs.board.current?.fit() : s.refs.graph.current?.fit()),
        messageAll: () => team && s.broadcast(team, []),
        messageSelected: () => team && s.broadcast(team, s.picked),
        interrupt: () => void s.refs.controls.current?.interrupt(),
        cycleAgent: (step) => cycleAgent({ shell: s, team, openAgent: d.openAgent }, step),
        back: d.back,
      },
    });
  };
}

type Command = ReturnType<typeof buildCommands>[number];

/** `]` and `[` on an agent: the next or previous agent of its team. */
export function cycleAgent(c: { shell: Shell; team: string | undefined; openAgent: string | undefined }, step: 1 | -1) {
  const list = c.shell.rows.find((r) => r.rig === c.team)?.agents ?? [];
  if (list.length < 2) return;
  const i = list.findIndex((a) => a.id === c.openAgent);
  c.shell.open(list[(i + step + list.length) % list.length]);
}
