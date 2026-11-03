import { expect, type APIRequestContext, type Locator, type Page } from '@playwright/test';

/**
 * The Practice team the Control specs run (control.spec.ts, control-video.spec.ts): a Planner who hands
 * work to a Buyer and an Inspector, with a rule that a purchase order over $5,000 needs a person. It is
 * made through the API with Practice agents (`runtime: stub`), so no model is ever called; the
 * agents' replies are the stub driver's scripts (crates/drivers/src/stub.rs).
 */

export const AGENTS = ['Planner', 'Buyer', 'Inspector'] as const;

export async function practiceTeam(request: APIRequestContext, name: string): Promise<string> {
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}-${Date.now()}`, template: 'blank', practice: true, name } });
  expect(created.ok(), await created.text()).toBe(true);
  const draft = (await created.json()).draft;
  const agent = (id: string, label: string, job: string, x: number, y: number) => ({ id, group_id: null, name: label, kind: 'practice', job, folder: null, x, y });
  draft.agents = [
    agent('planner', 'Planner', "Plan this week's upkeep for press line 4.", 60, 160),
    agent('buyer', 'Buyer', 'Order the parts the plan needs.', 400, 40),
    agent('inspector', 'Inspector', 'Inspect the presses the plan names.', 400, 300),
  ];
  draft.connections = [
    { id: 'c1', from: 'planner', to: 'buyer', kind: 'hands-work-to' },
    { id: 'c2', from: 'planner', to: 'inspector', kind: 'hands-work-to' },
  ];
  draft.mandate = { approve_within: true, rules: [{ id: 'limit', action: 'create_purchase_order', field: 'amount', test: { kind: 'at-most', value: 5000 } }] };
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${name}-${Date.now()}`, revision: draft.revision, draft } });
  expect(saved.ok(), await saved.text()).toBe(true);
  return draft.id;
}

/** An agent's id in a running team (agents outside a group are in `team`). */
export const seatOf = (team: string, agent: string) => `team.${agent}@${team}`;

/** Sends an agent a message as its operator would; Practice agents act on the stub's scripts. */
export async function say(request: APIRequestContext, team: string, agent: string, text: string) {
  const sent = await request.post(`/api/seats/${encodeURIComponent(seatOf(team, agent))}/send`, { data: { request_id: `say-${agent}-${Date.now()}`, text, intent: 'inform' } });
  expect(sent.ok(), await sent.text()).toBe(true);
}

/** The stub's script for a purchase order the Buyer proposes (over the $5,000 rule, so it waits for a person). */
export const ORDER = `propose_change:${JSON.stringify({
  action: 'create_purchase_order', system: 'ERP', summary: 'Order 2 spindle bearings SP-118 from Acme Hydraulics, $7,300',
  payload: { amount: 7300, supplier: 'Acme Hydraulics', item: 'SP-118', quantity: 2 }, reason: 'Press 4 is down until the bearings arrive.',
})}`;

export async function centre(l: Locator) {
  const b = (await l.boundingBox())!;
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}

/** Drags with the mouse, past the workspace's 4 px threshold, in steps a viewer can follow. */
export async function drag(page: Page, from: { x: number; y: number }, to: { x: number; y: number }, steps = 14) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 8, from.y + 8, { steps: 2 });
  await page.mouse.move(to.x, to.y, { steps });
  await page.mouse.up();
}

/** A stack (a group of tabbed panes) by one of its pane ids. */
export const stackOf = (page: Page, pane: string) => page.locator('section.ws-stack', { has: page.locator(`[data-tab="${pane}"]`) });

/** Started from Control's own Start; waits for every agent to be on the board. */
export async function startFromControl(page: Page) {
  await page.getByRole('group', { name: 'Team controls' }).getByRole('button', { name: 'Start' }).click();
  const overview = stackOf(page, 'live-overview');
  for (const a of AGENTS) await expect(overview.getByRole('button', { name: new RegExp(`^${a} — `) })).toBeVisible({ timeout: 30_000 });
}
