import { expect, test, type APIRequestContext } from '@playwright/test';
import { claudeAgent, confirmStart, liveOnly, liveTeam, TURN_MS } from './fixtures/live';

/**
 * Proposed changes in the real app, with a real Claude Code agent on haiku (live, see fixtures/live.ts):
 * a buyer proposes a purchase order over the team's limit with its propose_change tool, it waits for a
 * person in the project's Questions, the person approves it, and Results → Changes shows the change, the
 * rules as Sculptured checked them and who approved it. A second order within the rules is approved by
 * them without asking. Screenshots land in web/test-results/screens/changes/.
 */

liveOnly();

const SHOTS = 'test-results/screens/changes';

const order = (amount: number, supplier: string) => {
  const args = {
    action: 'create_purchase_order', system: 'ERP', summary: `Order 2 spindle bearings SP-118 from ${supplier}, $${amount}`,
    payload: { amount, supplier, item: 'SP-118', quantity: 2, needed_by: '2026-10-29' }, reason: 'Press 4 is down until the bearings arrive.',
  };
  return `Call your propose_change tool exactly once, with exactly these arguments, then reply with the single word OK. Do nothing else.\n${JSON.stringify(args)}`;
};

async function team(request: APIRequestContext, name: string, job: string): Promise<string> {
  const { id } = await liveTeam(request, name, (draft, folder) => {
    draft.agents = [claudeAgent('buyer', 'Buyer', job, folder, 60, 80)];
    draft.connections = [];
    draft.mandate = { approve_within: true, rules: [
      { id: 'limit', action: 'create_purchase_order', field: 'amount', test: { kind: 'at-most', value: 5000 } },
      { id: 'vendor', action: 'create_purchase_order', field: 'supplier', test: { kind: 'one-of', values: ['Acme Hydraulics', 'Bolt & Co'] } },
    ] };
  });
  return id;
}

test('an order over the limit waits for a person; Results → Changes shows who approved it', async ({ page, request }, info) => {
  test.setTimeout(2 * TURN_MS);
  const id = await team(request, `Changes ${Date.now()}-${info.project.name}`, order(7300, 'Acme Hydraulics'));
  await page.goto(`/p/${id}/settings`);
  await expect(page.getByRole('region', { name: 'Changes to business systems' })).toBeVisible();
  await page.getByRole('region', { name: 'Changes to business systems' }).screenshot({ path: `${SHOTS}/settings-rules-${info.project.name}.png` });

  await page.goto(`/team/${id}/structure`);
  await page.getByRole('button', { name: 'Start team' }).click();
  await confirmStart(page);
  await page.goto(`/p/${id}/questions`);
  await expect(page.getByText('Buyer proposes a change')).toBeVisible({ timeout: TURN_MS });
  await expect(page.getByText(/✗ fails: amount is 7300/)).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/approval-${info.project.name}.png`, fullPage: true });
  await page.getByRole('button', { name: 'Approve' }).click();

  await page.goto(`/p/${id}/results/changes`);
  await expect(page.getByText('✓Approved by a person').or(page.getByText('Approved by a person'))).toBeVisible({ timeout: 60_000 });
  await page.getByText('The change and its rules').click();
  await page.screenshot({ path: `${SHOTS}/results-changes-${info.project.name}.png`, fullPage: true });
  await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
});

test('an order within the rules is approved by them without asking anyone', async ({ request }, info) => {
  test.setTimeout(TURN_MS + 60_000);
  const id = await team(request, `Within ${Date.now()}-${info.project.name}`, order(4200, 'Bolt & Co'));
  const started = await request.post(`/api/teams/${id}/start`, { data: { request_id: `start-${id}` } });
  expect(started.ok()).toBe(true);
  await expect.poll(async () => (await (await request.get(`/api/teams/${id}/changes`)).json()).changes[0]?.decision?.by ?? null, { timeout: TURN_MS }).toBe('mandate');
  await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
});
