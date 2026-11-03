import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ArenaSection } from '../arena/routes';
import { openRigWords } from '../test-vocab';
import { REFUSAL, arenaFake, branchB2, resetArena, at } from './simHarness';
import { SimulatedPanePage } from './SimulatedControlPage';
import { ARENA_CANT_CONTROL } from './simulatedSource';

beforeEach(resetArena);
afterEach(() => vi.unstubAllGlobals());

function Where() {
  const { pathname, search } = useLocation();
  return <output data-testid="at">{pathname + search}</output>;
}

const page = (path = '/arena/runs/r1/b1') => render(
  <MemoryRouter initialEntries={[path]}>
    <Routes><Route path="/arena/*" element={<><ArenaSection /><Where /></>} /></Routes>
  </MemoryRouter>,
);
const pane = (title: string) => screen.getByRole('region', { name: title });
const layoutTab = (name: string) => within(screen.getByRole('tablist', { name: 'Layouts' })).getByRole('tab', { name });
const steps = (agent: string) => within(pane('Timeline')).getByRole('group', { name: `${agent} steps` });
const at_ = () => screen.getByTestId('at').textContent;
const inspector = () => within(screen.getByRole('complementary', { name: 'Selected step' }));
/** The step the replay's cursor is on, from its controls. */
const cursorAt = () => within(screen.getByRole('toolbar', { name: 'Replay controls' })).getByText(/^Step/).textContent;

async function loaded(path?: string) {
  const r = page(path);
  await screen.findByRole('heading', { name: /Task 27/ });
  return r;
}

describe('A simulated run in the Control workspace', () => {
  it('opens on Replay, with the run, its outcome and controls that say why they are off', async () => {
    const { container } = await loaded();
    expect(within(screen.getByRole('tablist', { name: 'Layouts' })).getAllByRole('tab').map((t) => t.textContent)).toEqual(['Replay', 'Live', 'Cost']);
    expect(layoutTab('Replay')).toHaveAttribute('aria-selected', 'true');
    for (const t of ['Timeline', 'World state', 'Compare']) expect(pane(t)).toBeInTheDocument();
    const heading = screen.getByRole('heading', { name: /Task 27 · Original/ });
    expect(within(heading).getByText('Passed the check')).toHaveTextContent('✓Passed the check');
    const strip = within(screen.getByRole('list', { name: 'Run status' }));
    expect(strip.getByText('passed the check')).toBeInTheDocument();
    expect(strip.getByText('300 total')).toBeInTheDocument();
    expect(strip.getByText('00:00:20')).toBeInTheDocument();
    const controls = within(screen.getByRole('group', { name: 'Team controls' }));
    // The four stop controls share one reason: said once, and kept as each one's description.
    for (const name of ['Pause all', 'Resume all', 'Stop team…', 'Emergency stop']) {
      const button = controls.getByRole('button', { name: new RegExp(name) });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription(`: ${ARENA_CANT_CONTROL}`);
    }
    expect(controls.getAllByText(ARENA_CANT_CONTROL)).toHaveLength(1);
    expect(controls.getByText('Start runs from Arena control')).toBeInTheDocument();
    expect(controls.getByRole('link', { name: 'Open Arena control' })).toHaveAttribute('href', '/arena');
    expect(screen.queryByText('Tests')).toBeNull();
    expect(openRigWords(container)).toEqual([]);
  });

  it('timeline_steps_with_keys', async () => {
    const user = userEvent.setup();
    await loaded();
    // The cursor starts on the latest step; a step's mark moves it there and opens the step.
    expect(cursorAt()).toMatch(/^Step 11/);
    await user.click(within(steps('Resolver')).getByRole('button', { name: /^Step 7: exchange_delivered_order_items/ }));
    expect(at_()).toBe('/arena/runs/r1/b1?step=7');
    expect(inspector().getByText('exchange_delivered_order_items')).toBeInTheDocument();
    expect(await inspector().findByText(/^- "status": "delivered"$/)).toBeInTheDocument();
    expect(inspector().getByText(/^\+ "status": "exchange requested"$/)).toBeInTheDocument();
    expect(inspector().getByText(/^Saved after this step: plant, retail/)).toBeInTheDocument();
    // ←/→ and j/k step through the steps in order; Home goes to the first.
    for (const [key, step] of [['{ArrowRight}', 8], ['j', 9], ['k', 8], ['{ArrowLeft}', 7], ['{ArrowLeft}', 6], ['{Home}', 1], ['{End}', 11]] as const) {
      await user.keyboard(key);
      expect(cursorAt()).toMatch(new RegExp(`^Step ${step} `));
    }
    expect(at_()).toBe('/arena/runs/r1/b1?step=11');
    expect(within(steps('Check')).getByRole('button', { name: /^Step 11: Check passed/ })).toHaveAttribute('aria-pressed', 'true');
  });

  it('fork_shows_plan_and_navigates', async () => {
    const user = userEvent.setup();
    await loaded('/arena/runs/r1/b1?step=7');
    await user.click(within(pane('Timeline')).getByRole('button', { name: /Fork from here/ }));
    const panel = within(screen.getByRole('region', { name: 'Fork from step 7' }));
    expect(await panel.findByText('retail: from its save')).toBeInTheDocument();
    expect(panel.getByText('plant: replayed from the start, about 40 s')).toBeInTheDocument();
    expect(panel.getByText('desk: from its save')).toBeInTheDocument();
    expect(panel.getByRole('button', { name: 'Exact' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(panel.getByRole('button', { name: 'Approximate' }));
    expect(await panel.findByText('plant: approximate')).toBeInTheDocument();
    const box = panel.getAllByRole('textbox', { name: 'Instructions' })[1];
    await user.clear(box);
    await user.type(box, 'Be careful.');
    await user.click(panel.getByRole('button', { name: 'Fork with 1 change' }));
    const fork = arenaFake.calls.find((c) => c.method === 'POST');
    expect(fork).toMatchObject({ url: '/api/arena/branches/b1/fork', body: { at_seq: 7, changes: { resolver: { prompt: 'Be careful.' } }, restore: 'approximate' } });
    // The new branch's workspace: where it started, and that it was restored approximately.
    await waitFor(() => expect(at_()).toBe('/arena/runs/r1/b2'));
    expect(await screen.findByText('Forked from step 7.')).toBeInTheDocument();
    const heading = await screen.findByRole('heading', { name: /Task 27 · Rerun 1/ });
    expect(within(heading).getByText('Approximate restore')).toBeInTheDocument();
    const bar = await screen.findByRole('navigation', { name: 'Branches of this run' });
    expect(within(bar).getAllByText('Approximate restore')).toHaveLength(1);
    expect(within(bar).getByRole('link', { name: /Rerun 1/ })).toHaveAttribute('aria-current', 'page');
  });

  it('an exact restore that is refused disables the fork, saying why', async () => {
    arenaFake.plans.exact = [409, { error: { code: 'arena_busy', message: REFUSAL, raw: REFUSAL } }];
    const user = userEvent.setup();
    await loaded('/arena/runs/r1/b1?step=7');
    await user.click(within(pane('Timeline')).getByRole('button', { name: /Fork from here/ }));
    const panel = within(screen.getByRole('region', { name: 'Fork from step 7' }));
    expect(await panel.findByText(REFUSAL)).toBeInTheDocument();
    expect(panel.getByRole('button', { name: 'Fork unchanged' })).toBeDisabled();
    await user.click(panel.getByRole('button', { name: 'Approximate' }));
    expect(await panel.findByText('plant: approximate')).toBeInTheDocument();
    expect(panel.queryByText(REFUSAL)).toBeNull();
    expect(panel.getByRole('button', { name: 'Fork unchanged' })).toBeEnabled();
  });

  it('the final check cannot be forked from, and says why', async () => {
    await loaded();
    expect(within(pane('Timeline')).getByRole('button', { name: /Fork from here/ })).toBeDisabled();
    expect(within(pane('Timeline')).getByText('The final check can’t be forked from; pick a step an agent took.')).toBeInTheDocument();
  });

  it('world_rows_at_cursor', async () => {
    const user = userEvent.setup();
    await loaded('/arena/runs/r1/b1?step=4');
    const world = within(pane('World state'));
    // Step 4 saved nothing: the rows are the latest save's, after step 3, of every key touched by then.
    expect(await world.findByText(/the latest save at or before step 4/)).toBeInTheDocument();
    expect(await world.findByRole('button', { name: 'orders/#W1' })).toBeInTheDocument();
    expect(world.getByText(/"status": "delivered"/)).toHaveAttribute('data-raw-evidence');
    const reads = arenaFake.calls.filter((c) => c.url.includes('/rows?seq=3')).map((c) => c.url);
    expect(reads).toEqual(expect.arrayContaining([expect.stringContaining('&part=retail'), expect.stringContaining('&part=desk')]));
    // A key from the selection: only that row, at the cursor's save.
    await user.click(world.getByRole('button', { name: 'orders/#W1' }));
    await user.click(within(steps('Resolver')).getByRole('button', { name: /^Step 8: propose_change/ }));
    expect(await world.findByText(/"status": "exchange requested"/)).toBeInTheDocument();
    expect(world.queryByRole('button', { name: 'change/c1' })).toBeNull();
    expect(arenaFake.calls.at(-1)?.url).toBe('/api/arena/branches/b1/rows?seq=8&keys=orders%2F%23W1&part=retail');
  });

  it('simulated_arena_down_is_plain', async () => {
    arenaFake.down = true;
    const { container } = page();
    const banner = await screen.findByRole('alert');
    expect(banner).toHaveTextContent('Arena is not running');
    expect(within(banner).getByText('cd core && cargo run -p arenad')).toBeInTheDocument();
    for (const title of ['Timeline', 'World state', 'Compare']) expect(await within(pane(title)).findByText(/Offline: Arena is not running/)).toBeInTheDocument();
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(within(screen.getByRole('list', { name: 'Run status' })).getAllByRole('listitem')[0]).toHaveTextContent('Run∅ unknown');
    for (const n of within(container).getAllByText(/connection refused/)) expect(n).toHaveAttribute('data-raw-evidence');
    expect(openRigWords(container)).toEqual([]);
  });

  it('the list view lists every step in order', async () => {
    const user = userEvent.setup();
    await loaded();
    await user.click(within(pane('Timeline')).getByRole('button', { name: 'List' }));
    const rows = within(pane('Timeline')).getAllByRole('row').slice(1);
    expect(rows.map((r) => r.firstChild?.textContent)).toEqual(['1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11']);
  });

  it('compares a fork with its parent', async () => {
    arenaFake.branches.b2 = branchB2();
    arenaFake.runs = [{ id: 'r1', task_id: '27', created_at: at(0), branches: [arenaFake.branches.b1, branchB2()] }];
    await loaded('/arena/runs/r1/b2');
    const compare = within(pane('Compare'));
    expect(await compare.findByText('They first differ at step 7')).toBeInTheDocument();
    expect(compare.getByRole('combobox', { name: 'Compare with' })).toHaveValue('b1');
    expect(compare.getByText('Final world: 1 row differ')).toBeInTheDocument();
    expect(compare.getByRole('link', { name: 'Open the full comparison' })).toHaveAttribute('href', '/arena/compare/b1/b2');
  });

  it('on Live: the agents, the world parts with their restore words, and approvals from the world', async () => {
    const user = userEvent.setup();
    await loaded();
    await user.click(layoutTab('Live'));
    const overview = within(pane('System overview'));
    expect(overview.getByRole('button', { name: 'Triage' })).toBeInTheDocument();
    const part = (name: string) => within(overview.getByRole('listitem', { name: `Part ${name}` }));
    expect(await part('retail').findByText('Exact from a save')).toBeInTheDocument();
    expect(part('plant').getByText('Exact by replay')).toBeInTheDocument();
    expect(part('plant').getByText('Approximate')).toBeInTheDocument();
    expect(part('plant').getByText('◷ Arena clock')).toBeInTheDocument();
    expect(part('desk').getByText(/after step/)).toHaveTextContent('◆ after step 9');
    await user.click(screen.getByRole('tab', { name: /Approvals/ }));
    const approvals = within(pane('Approvals'));
    expect(approvals.getByText('Refund order #W1')).toHaveAttribute('data-raw-evidence');
    expect(approvals.getByText(/decided by its agents/)).toBeInTheDocument();
    expect(approvals.queryByRole('button', { name: 'Approve' })).toBeNull();
  });

  it('a popped-out pane opens on its own at the run’s pane address, sharing the Replay layout', async () => {
    const main = await loaded('/arena/runs/r1/b1?step=8');
    main.unmount();
    render(
      <MemoryRouter initialEntries={['/arena/runs/r1/b1/pane/replay-world?step=8']}>
        <Routes><Route path="/arena/runs/:run/:branch/pane/:instance" element={<SimulatedPanePage />} /></Routes>
      </MemoryRouter>,
    );
    expect(await screen.findByRole('heading', { name: 'World state' })).toBeInTheDocument();
    const world = within(screen.getByRole('main', { name: 'Simulated run pane' }));
    expect(await world.findByText(/As saved after step/)).toHaveTextContent('As saved after step 8');
    expect(await world.findByRole('button', { name: 'change/c2' })).toBeInTheDocument();
  });
});
