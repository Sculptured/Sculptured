import { expect, test, type Locator, type Page } from '@playwright/test';
import { centre, controlTeam, drag, stackOf, startFromControl } from './fixtures/control-team';
import { confirmStart, liveOnly } from './fixtures/live';

/**
 * Control against the real binary with real Claude Code agents on haiku (crates/e2e-server; live,
 * see fixtures/live.ts): the project opens on Control with the live board first; Start; drag a pane
 * beside another and see the layout kept after a reload; the keyboard "Move to…" path; Pause all and
 * Resume all; Emergency stop with its confirm; and on a phone, the pane switcher.
 */

liveOnly();

const box = async (l: Locator) => (await l.boundingBox())!;

/** Status sits right of the Timeline, beside it (their heights overlap), once the layout has settled. */
async function statusBesideTimeline(page: Page) {
  await expect(async () => {
    const status = await box(stackOf(page, 'live-status'));
    const timeline = await box(stackOf(page, 'live-timeline'));
    expect(status.x).toBeGreaterThan(timeline.x + timeline.width / 2);
    expect(status.y).toBeLessThan(timeline.y + timeline.height);
    expect(timeline.y).toBeLessThan(status.y + status.height);
  }).toPass({ timeout: 5000 });
}

/** Presses ↓ in the open menu until `name` has focus. */
async function arrowTo(page: Page, name: string) {
  const item = page.getByRole('menuitem', { name, exact: true });
  for (let i = 0; i < 40 && !(await item.evaluate((el) => el === document.activeElement)); i++) await page.keyboard.press('ArrowDown');
  await expect(item).toBeFocused();
}

/** The header's outcome note after an action (role status, or alert on a failure). */
const outcome = (page: Page) => page.locator('.ctl-outcome');

async function dismissToasts(page: Page) {
  for (const close of await page.locator('.toast-x').all()) await close.click({ timeout: 1000 }).catch(() => {});
}

test('desktop: Control opens first, starts, keeps a dragged layout, moves by keyboard, pauses, resumes and stops', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  test.setTimeout(300_000);
  const id = await controlTeam(request, `Control ${Date.now()}`);

  // The project opens on Control, whose first pane is the System overview (the live board).
  await page.goto(`/p/${id}`);
  await expect(page).toHaveURL(new RegExp(`/p/${id}/control$`));
  await expect(page.getByRole('region', { name: 'Control' }).or(page.getByRole('main', { name: 'Control' }))).toBeVisible();
  await expect(page.locator('[data-tab="live-overview"]')).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('list', { name: 'Run status' })).toContainText('not started');

  await startFromControl(page);
  await expect(page.getByRole('list', { name: 'Run status' })).toContainText('running');
  await dismissToasts(page);

  // Drag the Status tab to the right edge of the Timeline; the layout is kept after a reload.
  const timeline = await box(stackOf(page, 'live-timeline'));
  await drag(page, await centre(page.locator('[data-tab="live-status"]')), { x: timeline.x + timeline.width - 12, y: timeline.y + timeline.height / 2 });
  await statusBesideTimeline(page);
  await page.reload();
  await expect(page.locator('[data-tab="live-status"]')).toBeVisible();
  await statusBesideTimeline(page);

  // The keyboard path: the Timeline's ⋯ menu, Move to…, "As a tab with System overview".
  await page.getByRole('button', { name: 'Timeline options' }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('menuitem', { name: 'Move to…' })).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await arrowTo(page, 'As a tab with System overview');
  await page.keyboard.press('Enter');
  await expect(stackOf(page, 'live-overview').locator('[data-tab="live-timeline"]')).toBeVisible();
  await expect(page.locator('section.ws-stack')).toHaveCount(3);

  // Pause all, then Resume all: each says what it did.
  const controls = page.getByRole('group', { name: 'Team controls' });
  await controls.getByRole('button', { name: 'Pause all' }).click();
  await expect(outcome(page)).toContainText('Paused 3 agents', { timeout: 60_000 });
  const strip = page.getByRole('list', { name: 'Run status' });
  await expect(strip).toContainText('3 paused', { timeout: 60_000 });
  await controls.getByRole('button', { name: 'Resume all' }).click();
  await expect(outcome(page)).toContainText('Resumed 3 agents', { timeout: 60_000 });

  // Emergency stop asks first (Cancel has focus), then stops every agent; after that nothing runs.
  await expect(strip).toContainText('3 running', { timeout: 60_000 });
  await controls.getByRole('button', { name: 'Emergency stop' }).click();
  const confirm = page.getByRole('alertdialog', { name: 'Emergency stop' });
  await expect(confirm).toContainText('Stop all 3 agents now?');
  await expect(confirm.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await confirm.getByRole('button', { name: 'Stop now' }).click();
  await expect(outcome(page)).toContainText('Stopping 3 agents now', { timeout: 60_000 });
  await expect(controls.getByRole('button', { name: 'Emergency stop' })).toBeDisabled({ timeout: 60_000 });
  await expect(strip).toContainText('3 stopped', { timeout: 60_000 });
});

test('mobile: one pane at a time, chosen from the pane switcher', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'mobile', 'phone layout');
  test.setTimeout(300_000);
  const id = await controlTeam(request, `Control phone ${Date.now()}`);
  await page.goto(`/p/${id}/control`);
  const pane = page.getByRole('combobox', { name: 'Pane' });
  await expect(pane).toBeVisible();
  // The live board first, the same panes as on a desktop.
  await expect(pane.locator('option:checked')).toHaveText('System overview');
  await expect(pane.locator('option')).toHaveText(['System overview', 'Raw events', 'Status', 'Approvals', 'Alarms', 'Timeline']);
  await pane.selectOption({ label: 'Timeline' });
  await expect(page.getByText('The timeline fills in once the team does something.')).toBeVisible();
  // The header keeps the run and Emergency stop on one line; the rest one step back.
  await expect(page.getByRole('button', { name: 'Emergency stop' })).toBeVisible();
  await page.getByText('Agents, tokens, cost and team controls').click();
  await page.getByRole('group', { name: 'Team controls' }).getByRole('button', { name: 'Start' }).click();
  await confirmStart(page);
  await pane.selectOption({ label: 'Status' });
  await expect(page.getByText(/Planner/).first()).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole('button', { name: 'Emergency stop' })).toBeEnabled({ timeout: 60_000 });
  // Leave nothing running for the other specs.
  await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
});
