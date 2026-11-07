import { expect, test } from '@playwright/test';
import { claudeAgent, liveOnly, liveTeam, OK_JOB, replyWith, TURN_MS } from './fixtures/live';

/**
 * Result checks in the real app, with real Claude Code agents on haiku (live, see fixtures/live.ts): the
 * buyer is asked to reply with a given result, whose JSON must carry a price at most 5000 before it
 * reaches the approver. A $7,300 result goes back to the buyer and is not handed on; a $4,200 one passes
 * and is. Screenshots land in web/test-results/screens/result-checks/.
 */

liveOnly();

const SHOTS = 'test-results/screens/result-checks';
const result = (price: number) => `Found the bearing.\n\`\`\`json\n{"part_number": "SP-118", "price": ${price}}\n\`\`\``;

test('a result that fails its checks goes back to the agent; one that passes is handed on', async ({ page, request }, info) => {
  test.setTimeout(4 * TURN_MS);
  const { id, draft } = await liveTeam(request, `Buying ${info.project.name}`, (d, folder) => {
    d.agents = [claudeAgent('buyer', 'Buyer', replyWith(result(7300)), folder, 60, 80), claudeAgent('approver', 'Approver', OK_JOB, folder, 360, 80)];
    d.agents[0].advanced = { result_checks: [
      { id: 'part', field: 'part_number', test: { kind: 'present' } },
      { id: 'price', field: 'price', test: { kind: 'at-most', value: 5000 } },
    ] };
    d.connections = [{ id: 'c1', from: 'buyer', to: 'approver', kind: 'hands-work-to' }];
  });
  const name = draft.name as string;

  await page.goto(`/p/${id}/agents/team.buyer/setup`);
  const checks = page.getByRole('region', { name: 'Result checks' });
  if (!(await checks.isVisible())) await page.locator('summary', { hasText: /^Advanced ·/ }).click();
  await expect(checks.getByLabel('Result check 2: value')).toHaveValue('5000');
  await checks.screenshot({ path: `${SHOTS}/setup-${info.project.name}.png` });

  expect((await request.post(`/api/teams/${id}/start`, { data: { request_id: `start-${name}-${Date.now()}` } })).ok()).toBe(true);
  await page.goto(`/p/${id}/agents/team.buyer/conversation`);
  await expect(page.getByText(/Its result did not pass its checks \(price is 7300, at most 5000\): sent back to it/).first()).toBeVisible({ timeout: TURN_MS });
  await expect(page.getByText(/not handed on; it waits for you/)).toBeVisible({ timeout: 2 * TURN_MS });
  await page.screenshot({ path: `${SHOTS}/conversation-failed-${info.project.name}.png`, fullPage: true });

  const buyer = `team.buyer@${id}`;
  expect((await request.post(`/api/seats/${encodeURIComponent(buyer)}/send`, { data: { request_id: `send-${name}-${Date.now()}`, text: replyWith(result(4200)), intent: 'inform' } })).ok()).toBe(true);
  await expect(page.getByText('✓ Its result passed its checks and was handed on')).toBeVisible({ timeout: TURN_MS });
  await page.goto(`/p/${id}/agents/team.approver/conversation`);
  await expect(page.getByText(/4200/).first()).toBeVisible({ timeout: 60_000 });
  await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
});
