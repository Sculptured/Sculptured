import { expect, test, type APIRequestContext } from '@playwright/test';

/**
 * Proposed changes in the real app, Practice agents only: a buyer proposes a purchase order over the team's
 * limit, it waits for a person in the project's Questions, the person approves it, and Results → Changes
 * shows the change, the rules as Sculptured checked them and who approved it. A second order within the
 * rules is approved by them without asking. Screenshots land in web/test-results/screens/changes/.
 */
const SHOTS = 'test-results/screens/changes';

const order = (amount: number, supplier: string) => `propose_change:${JSON.stringify({
  action: 'create_purchase_order', system: 'ERP', summary: `Order 2 spindle bearings SP-118 from ${supplier}, $${amount}`,
  payload: { amount, supplier, item: 'SP-118', quantity: 2, needed_by: '2026-10-29' }, reason: 'Press 4 is down until the bearings arrive.',
})}`;

async function team(request: APIRequestContext, name: string, job: string): Promise<string> {
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}`, template: 'blank', practice: true, name } });
  expect(created.ok()).toBe(true);
  const draft = (await created.json()).draft;
  draft.agents = [{ id: 'buyer', group_id: null, name: 'Buyer', kind: 'practice', job, folder: null, x: 60, y: 80 }];
  draft.connections = [];
  draft.mandate = { approve_within: true, rules: [
    { id: 'limit', action: 'create_purchase_order', field: 'amount', test: { kind: 'at-most', value: 5000 } },
    { id: 'vendor', action: 'create_purchase_order', field: 'supplier', test: { kind: 'one-of', values: ['Acme Hydraulics', 'Bolt & Co'] } },
  ] };
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${name}`, revision: draft.revision, draft } });
  expect(saved.ok()).toBe(true);
  return draft.id;
}

test('an order over the limit waits for a person; Results → Changes shows who approved it', async ({ page, request }, info) => {
  const id = await team(request, `Changes ${Date.now()}-${info.project.name}`, order(7300, 'Acme Hydraulics'));
  await page.goto(`/p/${id}/settings`);
  await expect(page.getByRole('region', { name: 'Changes to business systems' })).toBeVisible();
  await page.getByRole('region', { name: 'Changes to business systems' }).screenshot({ path: `${SHOTS}/settings-rules-${info.project.name}.png` });

  await page.goto(`/team/${id}/structure`);
  await page.getByRole('button', { name: 'Start team' }).click();
  await page.goto(`/p/${id}/questions`);
  await expect(page.getByText('Buyer proposes a change')).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText('✗ fails: amount is 7300, at most 5000')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/approval-${info.project.name}.png`, fullPage: true });
  await page.getByRole('button', { name: 'Approve' }).click();

  await page.goto(`/p/${id}/results/changes`);
  await expect(page.getByText('✓Approved by a person').or(page.getByText('Approved by a person'))).toBeVisible({ timeout: 30_000 });
  await page.getByText('The change and its rules').click();
  await page.screenshot({ path: `${SHOTS}/results-changes-${info.project.name}.png`, fullPage: true });
});

test('an order within the rules is approved by them without asking anyone', async ({ request }, info) => {
  const id = await team(request, `Within ${Date.now()}-${info.project.name}`, order(4200, 'Bolt & Co'));
  const started = await request.post(`/api/teams/${id}/start`, { data: { request_id: `start-${id}` } });
  expect(started.ok()).toBe(true);
  await expect.poll(async () => (await (await request.get(`/api/teams/${id}/changes`)).json()).changes[0]?.decision?.by ?? null, { timeout: 30_000 }).toBe('mandate');
});
