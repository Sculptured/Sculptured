import { expect, test } from '@playwright/test';

/**
 * Result checks in the real app, Practice agents only (they echo their messages, so the Job's JSON is the
 * result's): a buyer's result must carry a price at most 5000 before it reaches the approver. A $7,300
 * result goes back to the buyer and is not handed on; a $4,200 one passes and is. Screenshots land in
 * web/test-results/screens/result-checks/.
 */
const SHOTS = 'test-results/screens/result-checks';
const result = (price: number) => `Found the bearing.\n\`\`\`json\n{"part_number": "SP-118", "price": ${price}}\n\`\`\``;

test('a result that fails its checks goes back to the agent; one that passes is handed on', async ({ page, request }, info) => {
  const name = `Buying ${Date.now()}-${info.project.name}`;
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}`, template: 'blank', practice: true, name } });
  const draft = (await created.json()).draft;
  const agent = (id: string, label: string, job: string, x: number) => ({ id, group_id: null, name: label, kind: 'practice', job, folder: null, x, y: 80 });
  draft.agents = [agent('buyer', 'Buyer', result(7300), 60), agent('approver', 'Approver', 'Approve the purchase.', 360)];
  draft.agents[0].advanced = { result_checks: [
    { id: 'part', field: 'part_number', test: { kind: 'present' } },
    { id: 'price', field: 'price', test: { kind: 'at-most', value: 5000 } },
  ] };
  draft.connections = [{ id: 'c1', from: 'buyer', to: 'approver', kind: 'hands-work-to' }];
  expect((await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${name}`, revision: draft.revision, draft } })).ok()).toBe(true);

  await page.goto(`/p/${draft.id}/agents/team.buyer/setup`);
  const checks = page.getByRole('region', { name: 'Result checks' });
  if (!(await checks.isVisible())) await page.locator('summary', { hasText: /^Advanced ·/ }).click();
  await expect(checks.getByLabel('Result check 2: value')).toHaveValue('5000');
  await checks.screenshot({ path: `${SHOTS}/setup-${info.project.name}.png` });

  expect((await request.post(`/api/teams/${draft.id}/start`, { data: { request_id: `start-${name}` } })).ok()).toBe(true);
  await page.goto(`/p/${draft.id}/agents/team.buyer/conversation`);
  await expect(page.getByText(/Its result did not pass its checks \(price is 7300, at most 5000\): sent back to it/).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText(/not handed on; it waits for you/)).toBeVisible({ timeout: 30_000 });
  await page.screenshot({ path: `${SHOTS}/conversation-failed-${info.project.name}.png`, fullPage: true });

  const buyer = `team.buyer@${draft.id}`;
  expect((await request.post(`/api/seats/${encodeURIComponent(buyer)}/send`, { data: { request_id: `send-${name}`, text: result(4200), intent: 'inform' } })).ok()).toBe(true);
  await expect(page.getByText('✓ Its result passed its checks and was handed on')).toBeVisible({ timeout: 30_000 });
  await page.goto(`/p/${draft.id}/agents/team.approver/conversation`);
  await expect(page.getByText(/4200/).first()).toBeVisible({ timeout: 30_000 });
  await request.post(`/api/teams/${draft.id}/stop`, { data: { request_id: `stop-${name}` } });
});
