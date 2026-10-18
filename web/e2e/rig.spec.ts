import { expect, test, type Page } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = (name: string) => path.join(here, 'fixtures', name);

async function boot(page: Page, file: string, rig: string) {
  await page.goto('/new');
  // The expert start-from-a-file form is under Advanced on the all-teams page.
  const advanced = page.locator('details.home-advanced');
  await advanced.locator('summary').click();
  await expect(advanced).toHaveAttribute('open', '');
  await advanced.getByLabel('Team file').fill(fixture(file));
  await advanced.getByRole('button', { name: 'Start team' }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${rig}/board$`));
}

test('desktop: start a team, message an agent and stop it', async ({ page }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  await boot(page, 'e2e-desktop.yaml', 'e2e-desktop');

  const boxA = page.getByRole('button', { name: /^a — / });
  await expect(boxA).toBeVisible();
  await expect(page.getByRole('button', { name: /^b — / })).toBeVisible();

  // Connection labels are their own elements and are not clipped.
  const label = page.locator('.edge-label').first();
  await expect(label).toHaveText('hands work to');
  const lb = (await label.boundingBox())!;
  expect(lb.width).toBeGreaterThan(40);

  // Ruling 31: a long lifecycle reason must stay inside its box (real-browser layout check).
  const boxC = page.getByRole('button', { name: /^c — needs you/ });
  await expect(boxC).toBeVisible();
  const reason = boxC.locator('.reason');
  await expect(reason).toContainText('working directory does not exist');
  const cb = (await page.locator('.agent-box', { has: boxC }).boundingBox())!;
  const rb = (await reason.boundingBox())!;
  expect(rb.y).toBeGreaterThanOrEqual(cb.y);
  expect(rb.y + rb.height).toBeLessThanOrEqual(cb.y + cb.height);
  expect(rb.x).toBeGreaterThanOrEqual(cb.x);
  expect(rb.x + rb.width).toBeLessThanOrEqual(cb.x + cb.width);

  // The agent pane shows the full reason.
  const fullReason = (await reason.textContent())!.trim();
  await boxC.click();
  const paneC = page.getByRole('region', { name: 'Agent c' });
  await expect(paneC).toBeVisible();
  await expect(paneC.getByText(`Reason: ${fullReason}`)).toBeVisible();
  expect(fullReason).toContain('/nonexistent/sculptured-e2e/');
  expect(fullReason).toMatch(/lifecycle\/reason$/);

  // Esc returns to the same board.
  await page.keyboard.press('Escape');
  await expect(page).toHaveURL(/\/p\/e2e-desktop\/board$/);
  await boxA.click();
  await expect(page.getByRole('region', { name: 'Agent a' })).toBeVisible();
  await page.getByRole('textbox', { name: 'Message' }).fill('hello');
  await page.getByRole('textbox', { name: 'Message' }).press('Enter');
  const convo = page.getByRole('list', { name: 'Conversation' });
  await expect(convo.getByText('stub received: hello')).toBeVisible();
  await expect(convo.getByText('read', { exact: true })).toBeVisible();

  // Controls are on the agent's Activity tab.
  await page.getByRole('link', { name: 'Activity' }).click();
  await page.getByRole('button', { name: 'Hard stop…' }).click();
  await expect(page.getByText(/Stops a\./)).toBeVisible();
  await page.getByRole('button', { name: 'Confirm stop agent' }).click();
  await page.getByRole('link', { name: 'Agents' }).click();
  await expect(page.getByRole('row', { name: /a .*stopped/ })).toBeVisible();
});

test('mobile: list view shows agents and connections, tap opens the agent', async ({ page }, info) => {
  test.skip(info.project.name !== 'mobile', 'mobile flow');
  await boot(page, 'e2e-mobile.yaml', 'e2e-mobile');

  // The Agents tab is the list: every agent as a row, connections as sentences.
  await page.getByRole('link', { name: 'Agents' }).tap();
  const rowA = page.getByRole('table').getByRole('link', { name: /^a/ });
  await expect(rowA).toBeVisible();
  await expect(page.getByText('a hands work to b')).toBeVisible();

  await rowA.tap();
  await expect(page.getByRole('region', { name: 'Agent a' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Send' })).toBeVisible();
});
