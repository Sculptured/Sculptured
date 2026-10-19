import { beforeEach, describe, expect, it } from 'vitest';
import { readAppearance } from '../appearance';
import { buildCommands, type CommandContext } from './commands';

const noop = () => {};
const ctx = (): CommandContext => ({
  view: 'home', picked: 0, runningAgents: 0,
  projects: [], go: noop,
  act: new Proxy({}, { get: () => noop }) as CommandContext['act'],
});
const find = (id: string) => buildCommands(ctx()).find(c => c.id === id)!;

beforeEach(() => { localStorage.clear(); delete document.documentElement.dataset.look; delete document.documentElement.dataset.mode; });

describe('look commands', () => {
  it('switch the look and, in Standard, the colour mode; the one in use says so', () => {
    expect(find('app.look.coastal').disabled).toBe('In use');
    expect(find('app.mode.dark').disabled).toBe('Standard look only');
    find('app.look.standard').run();
    expect(document.documentElement.dataset.look).toBe('standard');
    expect(find('app.look.standard').disabled).toBe('In use');
    expect(find('app.mode.system').disabled).toBe('In use');
    find('app.mode.dark').run();
    expect(readAppearance()).toMatchObject({ look: 'standard', mode: 'dark' });
    expect(document.documentElement.dataset.mode).toBe('dark');
  });
});

describe('things moved out of view stay one Ctrl K away', () => {
  it('All projects and Account and usage, which the top bar also links', () => {
    const gone: string[] = [];
    const c = { ...ctx(), go: (to: string) => gone.push(to) };
    const cmds = buildCommands(c);
    cmds.find((x) => x.id === 'app.all-teams')!.run();
    cmds.find((x) => x.id === 'app.account')!.run();
    expect(gone).toEqual(['/', '/account']);
    expect(cmds.find((x) => x.id === 'app.account')!.title).toBe('Account and usage');
  });

  it('Docs opens the guides in a new tab, and typing help finds it', () => {
    const opened: unknown[][] = [];
    const real = window.open;
    window.open = ((...a: unknown[]) => { opened.push(a); return null; }) as typeof window.open;
    try { find('app.docs').run(); } finally { window.open = real; }
    expect(opened).toEqual([['https://docs.usesculptured.com', '_blank', 'noopener']]);
    expect(find('app.docs').words).toMatch(/help/);
  });

  it('the workflow list, only on a Design board whose team has a workflow', () => {
    let opened = 0;
    const board = (workflow: boolean) => ({ status: () => ({ canUndo: false, canRedo: false, startReason: null, lockReason: null, workflow }), workflowList: () => opened++ }) as unknown as CommandContext['board'];
    const at = (workflow: boolean) => buildCommands({ ...ctx(), view: 'structure', board: board(workflow) });
    expect(at(false).find((x) => x.id === 'board.workflow-list')).toBeUndefined();
    at(true).find((x) => x.id === 'board.workflow-list')!.run();
    expect(opened).toBe(1);
  });

  it('Hand over again in an agent, with the reason when it cannot run', () => {
    let asked = 0;
    const team = { id: 't', title: 'T', builtHere: true, started: true, running: true };
    const at = (reason: string | null) => buildCommands({ ...ctx(), view: 'agent', team, interruptReason: null, handOver: { reason, ask: () => asked++ } });
    const cmd = at(null).find((x) => x.id === 'agent.hand-over')!;
    expect(cmd.title).toBe('Hand over again…');
    cmd.run();
    expect(asked).toBe(1);
    expect(at('The team is not running.').find((x) => x.id === 'agent.hand-over')!.disabled).toBe('The team is not running.');
    expect(buildCommands({ ...ctx(), view: 'agent', team, handOver: null }).find((x) => x.id === 'agent.hand-over')).toBeUndefined();
  });
});

describe('every page of a project is one Ctrl K away', () => {
  const team = { id: 'shop', title: 'Shop', builtHere: true, started: true, running: false };
  it('opens the Board (Design or Run), Agents, Results, Settings and run history', () => {
    const gone: string[] = [];
    const cmds = buildCommands({ ...ctx(), view: 'home', team, go: (to: string) => gone.push(to) });
    for (const id of ['view.design', 'view.run', 'view.agents', 'view.results', 'view.settings', 'team.history']) cmds.find((x) => x.id === id)!.run();
    expect(gone).toEqual(['/p/shop/board?mode=design', '/p/shop/board?mode=run', '/p/shop/agents', '/p/shop/results', '/p/shop/settings', '/p/shop/results/history']);
  });

  it('on an agent: its other tabs, by name', () => {
    const gone: string[] = [];
    const cmds = buildCommands({ ...ctx(), view: 'agent', team, agent: { id: 'team.builder@shop', tab: 'conversation', draftOnly: false }, go: (to: string) => gone.push(to) });
    expect(cmds.find((x) => x.id === 'agent.conversation')).toBeUndefined();
    cmds.find((x) => x.id === 'agent.setup')!.run();
    cmds.find((x) => x.id === 'agent.activity')!.run();
    expect(gone).toEqual(['/p/shop/agents/team.builder/setup', '/p/shop/agents/team.builder/activity']);
  });

  it('Run waits for a first start, and says so', () => {
    const cmds = buildCommands({ ...ctx(), view: 'structure', team: { ...team, started: false } });
    expect(cmds.find((x) => x.id === 'view.run')!.disabled).toBe('Run opens once the team has started.');
  });
});
