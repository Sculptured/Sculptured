import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ArenaSection } from '../../arena/routes';
import { arenaFake, branchB1, ev, resetArena } from '../simHarness';

/** b1 with the run's world declaring what each part is and what its tools do, and one refused write. */
function declaredRun() {
  const b = branchB1();
  b.events[0]!.payload = {
    task_id: '27',
    world: [
      { part: 'retail', integration: 'mongo', system: { kind: 'erp', product: 'ERPNext', stands_in_for: 'SAP S/4HANA', simulated: false },
        tools: [{ name: 'get_order_details', effect: 'read' }, { name: 'exchange_delivered_order_items', effect: 'write' }] },
      { part: 'plant', integration: 'counter', system: { kind: 'plant', product: 'Isaac Sim', simulated: true } },
      { part: 'desk', integration: 'sculptured' },
    ],
  };
  b.events.splice(10, 0, ev(20, 17, 'resolver', 'tool_call', { name: 'exchange_delivered_order_items', args: { order_id: '#W1' }, result: null, error: 'refused: needs an approved change', part: 'retail', reads: [], writes: [] }));
  return b;
}

beforeEach(() => { resetArena(); arenaFake.branches.b1 = declaredRun(); });
afterEach(() => vi.unstubAllGlobals());

async function openProcess() {
  render(<MemoryRouter initialEntries={['/arena/runs/r1/b1']}><Routes><Route path="/arena/*" element={<ArenaSection />} /></Routes></MemoryRouter>);
  await screen.findByRole('heading', { name: /Task 27/ });
  await userEvent.click(within(screen.getByRole('tablist', { name: 'Layouts' })).getByRole('tab', { name: 'Process' }));
}
const pane = (title: string) => within(screen.getByRole('region', { name: title }));

describe('the control plane on an Arena run', () => {
  it('the process view shows agents as units and the systems as equipment, labelled with what they stand in for', async () => {
    await openProcess();
    const units = within(pane('Process view').getByRole('list', { name: 'Agents (units)' }));
    expect(units.getByText('Triage')).toBeInTheDocument();
    expect(units.getByText(/⊘ 1 refused or failed/)).toBeInTheDocument();
    const equipment = within(pane('Process view').getByRole('list', { name: 'Systems (equipment)' }));
    expect(equipment.getByText('ERPNext')).toBeInTheDocument();
    expect(equipment.getByText('stands in for SAP S/4HANA · real')).toBeInTheDocument();
    expect(equipment.getByText('Isaac Sim')).toBeInTheDocument();
    expect(equipment.getByText('simulated')).toBeInTheDocument();
    expect(equipment.getByText(/interlock · 1 permit waiting/)).toBeInTheDocument();
    expect(pane('Process view').getByText(/permit granted by person/)).toBeInTheDocument();
  });

  it('the action timeline marks each action by system and effect; a refusal stands out and its trace shows the evidence', async () => {
    await openProcess();
    const actions = pane('Action timeline');
    const refused = actions.getByRole('button', { name: /exchange_delivered_order_items on ERPNext, write, refused/ });
    expect(refused).toHaveTextContent('⊘ERP·W');
    expect(refused.className).toContain('is-abnormal');
    expect(actions.getByRole('button', { name: /get_order_details on ERPNext, read, ok/ })).toHaveTextContent('ERP·R');
    await userEvent.click(refused);
    const trace = within(actions.getByRole('region', { name: 'Action trace' }));
    for (const e of trace.getAllByText('refused: needs an approved change')) expect(e).toHaveAttribute('data-raw-evidence');
    expect(trace.getByText(/"order_id": "#W1"/)).toBeInTheDocument();
    expect(trace.getAllByText(/as the system declares/)).toHaveLength(2);
    await userEvent.click(actions.getByRole('checkbox', { name: 'Refused or failed only' }));
    expect(actions.getByText('1 of 7 actions')).toBeInTheDocument();
    await userEvent.click(actions.getByRole('button', { name: 'List' }));
    expect(within(actions.getByRole('table', { name: 'Actions, newest first' })).getAllByRole('row')).toHaveLength(2);
  });

  it('trends count actions per system, and say Arena does not record latency', async () => {
    await openProcess();
    await userEvent.click(screen.getByRole('tab', { name: /Trends/ }));
    const table = within(pane('Trends').getByRole('table', { name: 'Actions per minute by system' }));
    const erp = table.getByText('ERPNext').closest('tr')!;
    expect(within(erp).getAllByRole('cell').map((c) => c.textContent).slice(-3)).toEqual(['1', '2', '1']);
    expect(pane('Trends').getByText('Arena does not record how long a call took')).toBeInTheDocument();
  });
});
